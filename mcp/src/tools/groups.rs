use crate::{
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, report_link},
    report::{ReportType, SectionCtx, group::Group, strip_markup},
    server::FlyBaseServer,
    tools::{
        resolve::{normalize_fb_prefix, resolve_ids},
        search::MAX_OFFSET,
    },
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GroupsParams {
    /// A gene (FBgn) for its groups and pathways, or a gene group or pathway (FBgg) for its members and hierarchy.
    pub id: String,
    /// Max members to list for a group (default 25, max 100).
    pub limit: Option<u32>,
    /// Group members to skip; offset + limit may not exceed 10,000.
    pub offset: Option<u32>,
}

#[tool_router(router = tool_router_groups, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "get_gene_groups_and_pathways",
        description = "For a gene: the FlyBase gene groups and signaling pathways it belongs to. For a gene group or pathway ID (FBgg): its description, members, and parent and child groups. Use when a question is about families, complexes, or pathways; use get_report with an FBgg for references and GO terms too."
    )]
    pub async fn get_gene_groups_and_pathways(
        &self,
        Parameters(p): Parameters<GroupsParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_gene_groups_and_pathways";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.groups_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn groups_impl(&self, p: GroupsParams) -> ToolResult<CallToolResult> {
        const T: &str = "get_gene_groups_and_pathways";
        let input = normalize_fb_prefix(p.id.trim()).into_owned();
        if !(input.starts_with("FBgn") || input.starts_with("FBgg")) || input.len() != 11 {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "id",
                message: format!("'{input}' must be a gene (FBgn) or gene group (FBgg) ID"),
            });
        }
        let st = resolve_ids(&self.chado, T, std::slice::from_ref(&input))
            .await?
            .remove(0);
        let id = match st.status {
            "current" | "updated" => st.current_ids[0].clone(),
            _ => {
                return Err(ToolError::NotFound {
                    tool: T,
                    message: format!("{input} is not a current ID; use resolve"),
                });
            }
        };
        let limit = clamp_limit(p.limit);
        let offset = i64::from(p.offset.unwrap_or(0));
        if offset + limit > MAX_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!("offset + limit may not exceed {MAX_OFFSET}"),
            });
        }
        let ctx = SectionCtx {
            conn: self.chado.get(T).await?,
            tool: T,
            id: id.clone(),
            limit,
            fetch: limit + 1,
            offset,
        };
        if id.starts_with("FBgg") {
            let rt = Group;
            let mut sections = Map::new();
            for name in ["identity", "members", "hierarchy"] {
                sections.insert(name.to_string(), rt.section(&ctx, name).await?);
            }
            let label = sections["identity"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string();
            let kind = if sections["identity"]["is_pathway"] == true {
                "pathway"
            } else {
                "gene group"
            };
            return Ok(Envelope::new(T, &self.release)
                .summary(format!(
                    "{id} ({label}) is a {kind} with {} member(s) shown{}.",
                    sections["members"]["count_shown"],
                    if sections["members"]["has_more"] == true {
                        "; more available with offset"
                    } else {
                        ""
                    }
                ))
                .link("report", report_link(&id))
                .field("id", json!(id))
                .field("type", json!("gene_group"))
                .field("offset", json!(offset))
                .field("sections", Value::Object(sections))
                .finish());
        }
        let rows = ctx.query("groups/for_gene", &[&id]).await?;
        let all: Vec<Value> = rows
            .iter()
            .map(|r| {
                let gid: String = r.get("id");
                json!({
                    "id": gid, "symbol": r.get::<_, Option<String>>("symbol"), "name": r.get::<_, Option<String>>("name"),
                    "is_pathway": r.get::<_, Option<bool>>("is_pathway").unwrap_or(false),
                    "membership": r.get::<_, String>("membership"), "link": report_link(&gid),
                })
            })
            .collect();
        let (pathways, groups): (Vec<Value>, Vec<Value>) =
            all.into_iter().partition(|g| g["is_pathway"] == true);
        let symbol = st.symbol.map(|s| strip_markup(&s)).unwrap_or_default();
        Ok(Envelope::new(T, &self.release)
            .summary(format!(
                "{id} ({symbol}) is in {} pathway(s) and {} gene group(s).",
                pathways.len(),
                groups.len()
            ))
            .link("report", report_link(&id))
            .field("id", json!(id))
            .field("symbol", json!(symbol))
            .field("pathways", json!(pathways))
            .field("groups", json!(groups))
            .finish())
    }
}
