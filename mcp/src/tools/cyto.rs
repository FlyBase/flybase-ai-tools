use crate::{
    backends::chado::Chado,
    error::{ToolError, ToolResult},
    limits::clamp_limit,
    output::{Envelope, complete, report_link},
    report::jbrowse_link,
    server::FlyBaseServer,
    tools::{
        map::{FeatureType, MAX_COORD, Range, feature_row, overlaps},
        search::MAX_OFFSET,
        sequence::MAX_REGION,
    },
};
use deadpool_postgres::Transaction;
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CytoParams {
    /// A band (91A1), subdivision (91A), division (91), a band range on one arm (91A1-91B2), or a genomic range (3R:18268611..18297020).
    pub location: String,
    /// Feature types to return (default gene, insertion); see map_features list_options.
    pub feature_types: Option<Vec<FeatureType>>,
    /// Also list aberrations (deficiencies, duplications, inversions) whose cytological extent overlaps (default true). Only aberrations with mapped breakpoints appear.
    pub include_aberrations: Option<bool>,
    /// Keep features that only partly overlap the range (default true). When false, pages may be shorter than limit; follow has_more.
    pub include_partial: Option<bool>,
    /// Features per page (default 25, max 100); aberrations use the same page size.
    pub limit: Option<u32>,
    /// Features and aberrations to skip.
    pub offset: Option<u32>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Loc {
    Band(String),
    BandRange(String, String),
    Range {
        chromosome: String,
        start: u64,
        end: u64,
    },
}

/// Parse the location argument. Bands are `<division><letter>?<number>?` with the letter upper-cased.
pub fn parse_location(s: &str) -> Result<Loc, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("location is empty".into());
    }
    if let Some((chrom, rest)) = s.split_once(':') {
        let (a, b) = rest
            .split_once("..")
            .ok_or_else(|| format!("location '{s}' must look like 3R:18268611..18297020"))?;
        let start: u64 = a
            .trim()
            .replace(',', "")
            .parse()
            .map_err(|_| format!("location '{s}': start is not a number"))?;
        let end: u64 = b
            .trim()
            .replace(',', "")
            .parse()
            .map_err(|_| format!("location '{s}': end is not a number"))?;
        if !matches!(chrom, "X" | "Y" | "2L" | "2R" | "3L" | "3R" | "4") {
            return Err(format!(
                "location '{s}': chromosome arm must be one of X, Y, 2L, 2R, 3L, 3R, 4"
            ));
        }
        if start == 0 || end < start || end > MAX_COORD {
            return Err(format!(
                "location '{s}': end must be >= start, start >= 1, and end at most {MAX_COORD}"
            ));
        }
        return Ok(Loc::Range {
            chromosome: chrom.to_string(),
            start,
            end,
        });
    }
    let band = |b: &str| -> Result<String, String> {
        let b = b.trim().to_ascii_uppercase();
        let digits = b.bytes().take_while(u8::is_ascii_digit).count();
        let rest = &b[digits..];
        let ok = (1..=3).contains(&digits)
            && (rest.is_empty()
                || ((b'A'..=b'H').contains(&rest.as_bytes()[0])
                    && rest[1..].bytes().all(|c| c.is_ascii_digit())
                    && rest.len() <= 3));
        if ok {
            Ok(b)
        } else {
            Err(format!(
                "location '{s}': '{b}' is not a band such as 91A1, 91A, or 91"
            ))
        }
    };
    match s.split_once('-') {
        None => Ok(Loc::Band(band(s)?)),
        Some((a, b)) => Ok(Loc::BandRange(band(a)?, band(b)?)),
    }
}

struct Band {
    band: String,
    level: String,
    arm: String,
    fmin: i64,
    fmax: i64,
}

async fn band(tx: &Transaction<'_>, t: &'static str, name: &str) -> ToolResult<Band> {
    let rows = Chado::query_tx(tx, t, "cyto/band", &[&name]).await?;
    let r = rows.first().ok_or_else(|| ToolError::NotFound {
        tool: t,
        message: format!("no cytological band named '{name}' in the D. melanogaster map"),
    })?;
    Ok(Band {
        band: r.get("band"),
        level: r.get("level"),
        arm: r.get("arm"),
        fmin: i64::from(r.get::<_, i32>("fmin")),
        fmax: i64::from(r.get::<_, i32>("fmax")),
    })
}

#[tool_router(router = tool_router_cyto, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "cytosearch",
        description = "Find genes, insertions, other features, and aberrations in a cytological location: a band (91A1), subdivision (91A), division (91), band range (91A1-91B2), or genomic range (3R:18268611..18297020). Bands resolve to genome coordinates; aberrations are those with mapped breakpoints only; insertion sites are points (start = end). Use when a paper gives a cytological position; use map_features when you already have coordinates."
    )]
    pub async fn cytosearch(
        &self,
        Parameters(p): Parameters<CytoParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "cytosearch";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.heavy(T).await?;
            self.cyto_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    /// Everything runs inside one heavy transaction: band lookups, the overlap query, and the aberration query.
    async fn cyto_impl(&self, p: CytoParams) -> ToolResult<CallToolResult> {
        const T: &str = "cytosearch";
        let loc = parse_location(&p.location).map_err(|m| ToolError::InvalidInput {
            tool: T,
            arg: "location",
            message: m,
        })?;
        let limit = clamp_limit(p.limit);
        let offset = i64::from(p.offset.unwrap_or(0));
        if offset + limit > MAX_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!("offset + limit may not exceed {MAX_OFFSET}"),
            });
        }
        let types = p
            .feature_types
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| vec![FeatureType::Gene, FeatureType::Insertion]);
        let so_types = FeatureType::so_names(&types);
        let fetch = limit + 1;
        let include_partial = p.include_partial.unwrap_or(true);
        let include_aberrations = p.include_aberrations.unwrap_or(true);
        let location = p.location.trim().to_string();
        let (range, bands, rows, ab_rows) = self
            .chado
            .heavy(T, async |tx| {
                let (range, bands): (Range, Vec<Band>) = match loc {
                    Loc::Range {
                        chromosome,
                        start,
                        end,
                    } => (
                        Range {
                            species: "Dmel".into(),
                            chromosome,
                            fmin: start as i64 - 1,
                            fmax: end as i64,
                        },
                        Vec::new(),
                    ),
                    Loc::Band(b) => {
                        let b = band(tx, T, &b).await?;
                        (
                            Range {
                                species: "Dmel".into(),
                                chromosome: b.arm.clone(),
                                fmin: b.fmin,
                                fmax: b.fmax,
                            },
                            vec![b],
                        )
                    }
                    Loc::BandRange(a, b) => {
                        let (a, b) = (band(tx, T, &a).await?, band(tx, T, &b).await?);
                        if a.arm != b.arm {
                            return Err(ToolError::InvalidInput {
                                tool: T,
                                arg: "location",
                                message: format!(
                                    "{} is on {} but {} is on {}; a band range must stay on one arm",
                                    a.band, a.arm, b.band, b.arm
                                ),
                            });
                        }
                        let (fmin, fmax) = (a.fmin.min(b.fmin), a.fmax.max(b.fmax));
                        (
                            Range {
                                species: "Dmel".into(),
                                chromosome: a.arm.clone(),
                                fmin,
                                fmax,
                            },
                            vec![a, b],
                        )
                    }
                };
                if range.fmax - range.fmin > MAX_REGION as i64 * 4 {
                    return Err(ToolError::LimitExceeded {
                        tool: T,
                        message: format!(
                            "{location} spans {} bp; search a subdivision or band instead",
                            range.fmax - range.fmin
                        ),
                    });
                }
                let rows = overlaps(tx, T, &range, &so_types, fetch, offset).await?;
                let ab_rows = if include_aberrations {
                    Chado::query_tx(
                        tx,
                        T,
                        "cyto/aberrations",
                        &[
                            &range.chromosome,
                            &(range.fmin as i32),
                            &(range.fmax as i32),
                            &fetch,
                            &offset,
                        ],
                    )
                    .await?
                } else {
                    Vec::new()
                };
                Ok((range, bands, rows, ab_rows))
            })
            .await?;
        // Partial hits are filtered after the fetch, so a page may come back short when they are excluded; has_more still tells the caller to continue.
        let has_more = rows.len() as i64 > limit;
        let features: Vec<Value> = rows
            .iter()
            .take(limit as usize)
            .map(feature_row)
            .filter(|f| include_partial || f["overlap"] != "partial")
            .collect();
        let (aberrations, ab_more) = if include_aberrations {
            let rows = ab_rows;
            let more = rows.len() as i64 > limit;
            let list: Vec<Value> = rows
                .iter()
                .take(limit as usize)
                .map(|r| {
                    let id: String = r.get("id");
                    json!({"id": id, "symbol": r.get::<_, Option<String>>("symbol"), "start": r.get::<_, i32>("fmin") + 1, "end": r.get::<_, i32>("fmax"), "overlap": r.get::<_, String>("overlap"), "link": report_link(&id)})
                })
                .filter(|a| include_partial || a["overlap"] != "partial")
                .collect();
            (list, more)
        } else {
            (Vec::new(), false)
        };
        let resolved = json!({
            "chromosome": range.chromosome, "start": range.fmin + 1, "end": range.fmax,
            "bands": bands.iter().map(|b| json!({"band": b.band, "level": b.level, "start": b.fmin + 1, "end": b.fmax})).collect::<Vec<_>>(),
        });
        let summary = format!(
            "{} resolves to {}:{}..{}: {} feature(s){} and {} aberration(s){}.",
            p.location.trim(),
            range.chromosome,
            range.fmin + 1,
            range.fmax,
            features.len(),
            if has_more { " (more with offset)" } else { "" },
            aberrations.len(),
            if ab_more { " (more with offset)" } else { "" }
        );
        Ok(Envelope::new(T, &self.release)
            .summary(summary)
            .link(
                "jbrowse",
                jbrowse_link(&range.chromosome, range.fmin, range.fmax),
            )
            .field("resolved", resolved)
            .field("features", json!(features))
            .field("has_more", json!(has_more))
            .field("aberrations", json!(aberrations))
            .field("aberrations_has_more", json!(ab_more))
            .field("offset", json!(offset))
            .finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locations_parse() {
        assert_eq!(parse_location("91A1"), Ok(Loc::Band("91A1".into())));
        assert_eq!(parse_location(" 91a "), Ok(Loc::Band("91A".into())));
        assert_eq!(parse_location("91"), Ok(Loc::Band("91".into())));
        assert_eq!(
            parse_location("91A1-91B2"),
            Ok(Loc::BandRange("91A1".into(), "91B2".into()))
        );
        assert_eq!(
            parse_location("3R:18268611..18297020"),
            Ok(Loc::Range {
                chromosome: "3R".into(),
                start: 18268611,
                end: 18297020
            })
        );
        for bad in [
            "",
            "91Z",
            "91A1-",
            "3R:5..1",
            "chr3R:1..2",
            "91A12345",
            "A1",
            "3R:1-2",
            "3R:1..99999999999",
        ] {
            assert!(parse_location(bad).is_err(), "{bad}");
        }
    }
}
