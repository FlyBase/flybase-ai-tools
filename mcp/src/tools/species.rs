use crate::backends::cache::ResponseCache;
use crate::{
    error::ToolResult,
    limits::{MAX_JSON_BYTES, MAX_ROWS},
    output::{Envelope, complete},
    server::FlyBaseServer,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Deserialize, Serialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ListSpeciesParams {
    /// Include organisms without a genome assembly (default false).
    pub include_unsequenced: Option<bool>,
    /// Organisms per page (default 100 when paging, max 1000). With no limit or offset, the assembly-only list is unpaged. Wide rows can shorten pages.
    pub limit: Option<u32>,
    /// Organisms to skip in the filtered list (default 0). Use next_offset from the previous page.
    pub offset: Option<u32>,
}

// Source: https://flybase.org/maps/synteny (fetched 2026-10-11).
const SYNTENY: &str = include_str!("../../data/muller_synteny.tsv");

pub fn synteny_rows() -> Vec<serde_json::Value> {
    let mut lines = SYNTENY.lines();
    let header: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    lines
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            json!(
                header
                    .iter()
                    .zip(l.split('\t'))
                    .map(|(h, c)| (h.to_string(), json!(c)))
                    .collect::<serde_json::Map<_, _>>()
            )
        })
        .collect()
}

#[tool_router(router = tool_router_species, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "list_species",
        description = "List organisms in FlyBase with their abbreviations (Dmel, Dsim, ...) and the Muller element synteny table. By default, return all organisms with genome assemblies. Set include_unsequenced=true to page through all organisms with abbreviations. If has_more is true, repeat with the same filter and limit and pass next_offset as offset. organisms_total and assemblies_total count all organisms with abbreviations and those with assemblies, respectively, regardless of the current page. Use when you need a species code for another tool or want to know which genomes are available."
    )]
    pub async fn list_species(
        &self,
        Parameters(p): Parameters<ListSpeciesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "list_species";
        let started = std::time::Instant::now();
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, async {
                    let _permit = self.limits.light(T).await?;
                    self.list_species_impl(p).await
                })
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn list_species_impl(&self, p: ListSpeciesParams) -> ToolResult<CallToolResult> {
        const T: &str = "list_species";
        let include_all = p.include_unsequenced.unwrap_or(false);
        let offset = i64::from(p.offset.unwrap_or(0));
        let limit = (include_all || p.limit.is_some() || p.offset.is_some())
            .then(|| i64::from(p.limit.unwrap_or(100)).clamp(1, MAX_ROWS));
        let rows = self
            .chado
            .query(T, "species/list", &[&include_all, &limit, &offset])
            .await?;
        // The totals are present even when the requested page is empty.
        let total: i64 = rows[0].get("organisms_total");
        let with_assembly: i64 = rows[0].get("assemblies_total");
        let synteny = json!(synteny_rows());
        // Reserve room for the envelope and synteny before calculating the continuation offset.
        let mut remaining_bytes = MAX_JSON_BYTES.saturating_sub(
            1024 + json!(self.release).to_string().len() + synteny.to_string().len(),
        );
        let species: Vec<_> = rows
            .iter()
            .filter_map(|r| {
                let abbreviation = r.get::<_, Option<String>>("abbreviation")?;
                Some(json!({
                    "abbreviation": abbreviation,
                    "genus": r.get::<_, String>("genus"),
                    "species": r.get::<_, String>("species"),
                    "common_name": r.get::<_, Option<String>>("common_name"),
                    "has_assembly": r.get::<_, bool>("has_assembly"),
                }))
            })
            .take_while(|row| {
                let bytes = row.to_string().len() + 1;
                if bytes > remaining_bytes {
                    return false;
                }
                remaining_bytes -= bytes;
                true
            })
            .collect();
        let count_shown = species.len();
        let matched_total = if include_all { total } else { with_assembly };
        let next = offset + count_shown as i64;
        let has_more = next < matched_total;
        let continuation = if has_more {
            " Pass next_offset as offset for more."
        } else {
            ""
        };
        Ok(Envelope::new(T, &self.release)
            .summary(if include_all {
                format!(
                    "Showing {count_shown} of {total} organisms in FlyBase with abbreviations, {with_assembly} with genome assemblies.{continuation} Use the abbreviation (for example Dmel) as the species argument in other tools."
                )
            } else {
                format!(
                    "Showing {count_shown} of {with_assembly} species with genome assemblies ({total} organisms with abbreviations in total).{continuation} Use the abbreviation (for example Dmel) as the species argument in other tools."
                )
            })
            .field("species", json!(species))
            .field("organisms_total", json!(total))
            .field("assemblies_total", json!(with_assembly))
            .field("count_shown", json!(count_shown))
            .field("has_more", json!(has_more))
            .field("next_offset", json!(has_more.then_some(next)))
            .field("muller_synteny", synteny)
            .finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn synteny_parses() {
        let rows = synteny_rows();
        assert_eq!(rows.len(), 6);
        assert_eq!(rows[0]["Dmel"], "X");
        let row = |m: &str| {
            rows.iter()
                .find(|r| r["muller_element"] == m)
                .unwrap()
                .clone()
        };
        assert_eq!(row("D")["Dwil"], "XR");
        assert_eq!(row("F")["Dwil"], "3");
        assert_eq!(row("E")["Dvir"], "2");
        assert_eq!(row("C")["Dgri"], "2");
        assert_eq!(row("D")["Dpse"], "XR");
        assert_eq!(row("B")["Dyak"], "2");
        assert!(
            rows.iter()
                .all(|r| r.get("notes").is_some_and(|n| n.is_string()))
        );
    }
}
