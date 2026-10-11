use crate::{output::Envelope, server::FlyBaseServer};
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
        let _permit = self.limits.light().await;
        let rows = match self.chado.query("list_species", "species/list", &[]).await {
            Ok(r) => r,
            Err(e) => return Ok(e.into_result()),
        };
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
        Ok(Envelope::new("list_species", &self.release)
            .summary(format!(
                "{} species with genome assemblies ({} organisms in total). Use the abbreviation (for example Dmel) as the species argument in other tools.",
                species.len(),
                total
            ))
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
    }
}
