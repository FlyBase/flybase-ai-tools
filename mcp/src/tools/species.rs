use crate::{
    error::ToolResult,
    output::{Envelope, complete},
    server::FlyBaseServer,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ListSpeciesParams {
    /// Include organisms without a genome assembly (default false).
    pub include_unsequenced: Option<bool>,
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
        description = "List the sequenced species in FlyBase with their abbreviations (Dmel, Dsim, ...) and the Muller element synteny table. Use when you need a species code for another tool or want to know which genomes are available."
    )]
    pub async fn list_species(
        &self,
        Parameters(p): Parameters<ListSpeciesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "list_species";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.list_species_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn list_species_impl(&self, p: ListSpeciesParams) -> ToolResult<CallToolResult> {
        const T: &str = "list_species";
        let rows = self.chado.query(T, "species/list", &[]).await?;
        let include_all = p.include_unsequenced.unwrap_or(false);
        let species: Vec<_> = rows
            .iter()
            .filter(|r| include_all || r.get::<_, bool>("has_assembly"))
            .map(|r| {
                json!({
                    "abbreviation": r.get::<_, String>("abbreviation"),
                    "genus": r.get::<_, String>("genus"),
                    "species": r.get::<_, String>("species"),
                    "common_name": r.get::<_, Option<String>>("common_name"),
                    "has_assembly": r.get::<_, bool>("has_assembly"),
                })
            })
            .collect();
        let total = rows.len();
        let with_assembly = rows
            .iter()
            .filter(|r| r.get::<_, bool>("has_assembly"))
            .count();
        Ok(Envelope::new(T, &self.release)
            .summary(if include_all {
                format!(
                    "{} organisms in FlyBase, {} with genome assemblies. Use the abbreviation (for example Dmel) as the species argument in other tools.",
                    total, with_assembly
                )
            } else {
                format!(
                    "{} species with genome assemblies ({} organisms in total). Use the abbreviation (for example Dmel) as the species argument in other tools.",
                    species.len(),
                    total
                )
            })
            .field("species", json!(species))
            .field("organisms_total", json!(total))
            .field("muller_synteny", json!(synteny_rows()))
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
