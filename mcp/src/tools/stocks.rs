use crate::backends::cache::ResponseCache;
use crate::{
    backends::chado::Chado,
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, report_link},
    report::strip_markup,
    server::FlyBaseServer,
    tools::{
        resolve::{like_escape, looks_like_fb_id, normalize_fb_prefix, resolve_ids},
        search::MAX_OFFSET,
    },
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize, Serialize, JsonSchema, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Bloomington,
    Kyoto,
    Vdrc,
    NigFly,
    Korea,
    Flyorf,
    Ndssc,
}

impl Provider {
    pub const ALL: &[Provider] = &[
        Self::Bloomington,
        Self::Kyoto,
        Self::Vdrc,
        Self::NigFly,
        Self::Korea,
        Self::Flyorf,
        Self::Ndssc,
    ];
    /// The `stockcollection.uniquename` the stock report uses for the same centre.
    pub fn collection(self) -> &'static str {
        match self {
            Self::Bloomington => "Bloomington Drosophila Stock Center",
            Self::Kyoto => "Kyoto Stock Center",
            Self::Vdrc => "Vienna Drosophila Resource Center",
            Self::NigFly => "National Institute of Genetics Fly Stocks",
            Self::Korea => "Korea Drosophila Resource Center",
            Self::Flyorf => "FlyORF",
            Self::Ndssc => "National Drosophila Species Stock Center",
        }
    }
    /// The provider whose stockcollection name this is, so the FBst path speaks the same `provider` vocabulary.
    pub fn from_collection(name: &str) -> Option<Provider> {
        Self::ALL.iter().copied().find(|p| p.collection() == name)
    }
    /// The `center` value in gene.allele_stock and flybase.get_stocks.
    pub fn center(self) -> &'static str {
        match self {
            Self::Bloomington => "Bloomington",
            Self::Kyoto => "Kyoto",
            Self::Vdrc => "VDRC",
            Self::NigFly => "NIG-Fly",
            Self::Korea => "Korea",
            Self::Flyorf => "FlyORF",
            Self::Ndssc => "NDSSC",
        }
    }
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StocksParams {
    /// A gene (FBgn), allele (FBal), insertion (FBti), aberration (FBab), balancer (FBba) or balancer symbol (FM7c), construct (FBtp), or stock (FBst).
    pub id: String,
    /// Keep GAL4 driver stocks only: for a gene, stocks of Scer\GAL4 alleles driven by the gene's regulatory region; for an allele,
    /// insertion, aberration, balancer, or construct, stocks that also carry a Scer\GAL4 allele. Ignored for a stock ID.
    pub gal4_only: Option<bool>,
    /// Stock centre.
    pub provider: Option<Provider>,
    /// Stocks per page (default 25, max 100).
    pub limit: Option<u32>,
    /// Stocks to skip; offset + limit may not exceed 10,000.
    pub offset: Option<u32>,
}

fn stock_row(r: &tokio_postgres::Row) -> Value {
    let id: Option<String> = r.get("id");
    let mut v = json!({
        "id": id, "provider": r.get::<_, Option<String>>("provider"), "stock_number": r.get::<_, Option<String>>("stock_number"),
        "genotype": r.get::<_, Option<String>>("genotype").map(|s| strip_markup(&s)), "link": id.as_deref().map(report_link),
    });
    if r.columns().iter().any(|c| c.name() == "allele_id") {
        v["allele_id"] = json!(r.get::<_, Option<String>>("allele_id"));
        v["allele_symbol"] = json!(
            r.get::<_, Option<String>>("allele_symbol")
                .map(|s| strip_markup(&s))
        );
        v["via_regulatory_region"] = json!(r.get::<_, Option<bool>>("via_regulatory_region"));
    }
    v
}

#[tool_router(router = tool_router_stocks, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "get_stocks",
        description = "Fly stocks available from stock centres (Bloomington, Kyoto, VDRC, NIG-Fly, Korea, FlyORF, NDSSC) for a gene, allele, insertion, aberration, balancer, construct, or stock ID, with stock numbers and genotypes; optionally GAL4 drivers only or one provider. Use when someone wants to order or find flies; use get_report for the entity itself."
    )]
    pub async fn get_stocks(
        &self,
        Parameters(p): Parameters<StocksParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_stocks";
        let started = std::time::Instant::now();
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, async {
                    let _permit = self.limits.light(T).await?;
                    self.stocks_impl(p).await
                })
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn stocks_impl(&self, p: StocksParams) -> ToolResult<CallToolResult> {
        const T: &str = "get_stocks";
        const CLASSES: &[&str] = &["FBgn", "FBal", "FBti", "FBab", "FBba", "FBtp", "FBst"];
        let input = normalize_fb_prefix(p.id.trim()).into_owned();
        if input.is_empty() || input.len() > 200 {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "id",
                message: "must be 1 to 200 characters".into(),
            });
        }
        let limit = clamp_limit(p.limit);
        let offset = i64::from(p.offset.unwrap_or(0));
        if offset + limit > MAX_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!("offset + limit may not exceed {MAX_OFFSET}"),
            });
        }
        let fetch = limit + 1;
        let gal4 = p.gal4_only.unwrap_or(false);
        let provider: Option<String> = p.provider.map(|x| x.center().to_string());
        // A balancer symbol (FM7c, CyO, TM3) resolves through the name index before the ID path.
        let id = if looks_like_fb_id(&input) {
            if !CLASSES.contains(&&input[..4]) {
                return Err(ToolError::InvalidInput {
                    tool: T,
                    arg: "id",
                    message: format!(
                        "{input} is not a gene, allele, insertion, aberration, balancer, construct, or stock"
                    ),
                });
            }
            let st = resolve_ids(&self.chado, T, std::slice::from_ref(&input))
                .await?
                .remove(0);
            match st.status {
                "current" | "updated" => st.current_ids[0].clone(),
                _ => {
                    return Err(ToolError::NotFound {
                        tool: T,
                        message: format!("{input} is not a current FlyBase ID; use resolve"),
                    });
                }
            }
        } else {
            let rows = self
                .chado
                .query(
                    T,
                    "resolve/candidates",
                    &[
                        &input,
                        &None::<String>,
                        &Some("balancer".to_string()),
                        &like_escape(&input),
                    ],
                )
                .await?;
            let exact = rows.iter().find(|r| {
                r.get::<_, String>("name").eq_ignore_ascii_case(&input)
                    && r.get::<_, bool>("is_current")
            });
            match exact {
                Some(r) => r.get::<_, String>("entity_id"),
                None => {
                    return Err(ToolError::NotFound {
                        tool: T,
                        message: format!(
                            "'{input}' is not a balancer symbol or FlyBase ID; use resolve first"
                        ),
                    });
                }
            }
        };
        let class = &id[..4];
        if !CLASSES.contains(&class) {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "id",
                message: format!(
                    "{id} is not a gene, allele, insertion, aberration, balancer, construct, or stock"
                ),
            });
        }
        let conn = self.chado.get(T).await?;
        let mut env = Envelope::new(T, &self.release)
            .link("report", report_link(&id))
            .field("id", json!(id))
            .field("offset", json!(offset));
        let (rows, total): (Vec<tokio_postgres::Row>, Option<i64>) = match class {
            "FBgn" => {
                let rows = Chado::query_on(
                    &conn,
                    T,
                    "stocks/gene",
                    &[&id, &gal4, &provider, &fetch, &offset],
                )
                .await?;
                let n = Chado::query_on(&conn, T, "stocks/gene_count", &[&id, &gal4, &provider])
                    .await?;
                (rows, n.first().map(|r| r.get::<_, i64>("n")))
            }
            "FBal" => (
                Chado::query_on(
                    &conn,
                    T,
                    "stocks/allele",
                    &[&id, &gal4, &provider, &fetch, &offset],
                )
                .await?,
                None,
            ),
            "FBst" => {
                // The stock report knows the collection name; map it to the same `provider` word the other paths use,
                // apply the provider filter, and say that gal4_only has no meaning for a single stock.
                let rows = Chado::query_on(&conn, T, "report/stock/identity", &[&id]).await?;
                let r = rows.first().ok_or_else(|| ToolError::NotFound {
                    tool: T,
                    message: format!("{id} not found"),
                })?;
                let collection = r.get::<_, Option<String>>("provider");
                let center = collection
                    .as_deref()
                    .and_then(Provider::from_collection)
                    .map(Provider::center);
                let stocks: Vec<Value> = if provider
                    .as_deref()
                    .is_some_and(|want| center != Some(want))
                {
                    Vec::new()
                } else {
                    vec![json!({
                        "id": id, "provider": center, "provider_name": collection,
                        "stock_number": r.get::<_, Option<String>>("stock_number"),
                        "genotype": r.get::<_, Option<String>>("genotype").map(|s| strip_markup(&s)), "link": report_link(&id),
                    })]
                };
                let summary = match stocks.first() {
                    Some(st) => format!(
                        "{id} is stock {} at {}.{}",
                        st["stock_number"].as_str().unwrap_or("?"),
                        st["provider"].as_str().unwrap_or("?"),
                        if gal4 {
                            " gal4_only does not apply to a stock ID."
                        } else {
                            ""
                        }
                    ),
                    None => format!(
                        "{id} is not held by {}.",
                        provider.as_deref().unwrap_or("?")
                    ),
                };
                return Ok(env
                    .summary(summary)
                    .field("stocks", json!(stocks))
                    .field("has_more", json!(false))
                    .finish());
            }
            _ => (
                Chado::query_on(
                    &conn,
                    T,
                    "stocks/feature",
                    &[&id, &gal4, &provider, &fetch, &offset],
                )
                .await?,
                None,
            ),
        };
        let has_more = rows.len() as i64 > limit;
        let stocks: Vec<Value> = rows.iter().take(limit as usize).map(stock_row).collect();
        if matches!(class, "FBgn" | "FBal") {
            let r = Chado::query_on(&conn, T, "stocks/split_system", &[&id]).await?;
            let combos: Vec<Value> = r
                .iter()
                .map(|r| {
                    let cid: String = r.get("id");
                    json!({
                        "id": cid, "symbol": r.get::<_, Option<String>>("symbol").map(|s| strip_markup(&s)),
                        "stocks_count": r.get::<_, Option<i64>>("stocks_count"), "link": report_link(&cid),
                    })
                })
                .collect();
            env = env.field("split_system_combinations", json!(combos));
        }
        let filters = [
            gal4.then(|| "GAL4 drivers only".to_string()),
            provider.as_ref().map(|c| format!("provider {c}")),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
        let summary = format!(
            "{} stock(s) shown for {id}{}{}{}.",
            stocks.len(),
            match total {
                Some(n) => format!(" of {n}"),
                None => String::new(),
            },
            if has_more { " (more with offset)" } else { "" },
            if filters.is_empty() {
                String::new()
            } else {
                format!("; {filters}")
            }
        );
        if let Some(n) = total {
            env = env.field("total", json!(n));
        }
        Ok(env
            .summary(summary)
            .field("stocks", json!(stocks))
            .field("has_more", json!(has_more))
            .finish())
    }
}
