use crate::backends::chado::Chado;
use crate::error::{ToolError, ToolResult};

/// Map a caller-supplied species abbreviation to its canonical form ("dmel" -> "Dmel") by matching
/// `organism.abbreviation` case-insensitively. Anything else is invalid input that points at list_species.
pub async fn normalize_species(
    chado: &Chado,
    tool: &'static str,
    input: &str,
) -> ToolResult<String> {
    let input = input.trim();
    let rows = chado.query(tool, "species/normalize", &[&input]).await?;
    match rows.first() {
        Some(r) => Ok(r.get::<_, String>("abbreviation")),
        None => Err(ToolError::InvalidInput {
            tool,
            arg: "species",
            message: format!(
                "unknown species '{input}'; call list_species for valid abbreviations"
            ),
        }),
    }
}
