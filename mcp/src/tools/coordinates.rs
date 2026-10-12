use crate::{
    backends::coordinates::{self as maps, Status},
    error::{ToolError, ToolResult},
    limits::MAX_BATCH,
    output::{Envelope, complete},
    server::FlyBaseServer,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const CONVERTER_URL: &str = "https://flybase.org/convert/coordinates";

#[derive(Deserialize, Serialize, JsonSchema, Default)]
#[serde(deny_unknown_fields)]
pub struct ConvertCoordinatesParams {
    /// Species abbreviation. Only Dmel has a bundled map (default Dmel).
    pub species: Option<String>,
    /// Source assembly release number (default 5). Only Dmel release 5 is supported.
    pub from_release: Option<u8>,
    /// Target assembly release number (default 6).
    pub to_release: Option<u8>,
    /// Up to 500 positions or ranges: "2L:21485500..21485700", "X:330", "chr3R:3,020,320", "2R:1-10000".
    pub coordinates: Option<Vec<String>>,
    /// If true, return only the supported species and release pairs with their scaffold names.
    pub list_options: Option<bool>,
}

fn supported() -> String {
    maps::pairs()
        .iter()
        .map(|p| format!("{} {} -> {}", p.species, p.from_release, p.to_release))
        .collect::<Vec<_>>()
        .join(", ")
}

#[tool_router(router = tool_router_coordinates, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "convert_coordinates",
        description = "Convert genome coordinates between assembly releases using FlyBase's published maps (currently D. melanogaster release 5 to release 6). Each input gets a status: mapped (both ends convert to one scaffold; a range that spans regions of change is still mapped, but its length can change, so compare from_length with to_length and check regions_spanned), partial (an end falls in a region of change, the two ends land on different scaffolds or strands, or the blocks between them are not in order in the target; the mapped blocks are listed), unmapped, or invalid. Use when a paper or dataset gives release 5 (pre-2014) coordinates and you need current ones; use list_options=true to see supported pairs."
    )]
    pub async fn convert_coordinates(
        &self,
        Parameters(p): Parameters<ConvertCoordinatesParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "convert_coordinates";
        let started = std::time::Instant::now();
        let res = async {
            let _permit = self.limits.light(T).await?;
            self.convert_coordinates_impl(p).await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn convert_coordinates_impl(
        &self,
        p: ConvertCoordinatesParams,
    ) -> ToolResult<CallToolResult> {
        const T: &str = "convert_coordinates";
        if p.list_options.unwrap_or(false) {
            let pairs: Vec<Value> = maps::MAPS
                .iter()
                .map(|m| json!({"species": m.pair.species, "from_release": m.pair.from_release, "to_release": m.pair.to_release, "source": m.source, "chromosomes": m.chromosomes()}))
                .collect();
            return Ok(Envelope::new(T, &self.release)
                .summary(format!("Supported conversions: {}.", supported()))
                .field("pairs", json!(pairs))
                .link("converter", CONVERTER_URL.into())
                .finish());
        }
        let species = p.species.as_deref().unwrap_or("Dmel").trim();
        let (from, to) = (p.from_release.unwrap_or(5), p.to_release.unwrap_or(6));
        let map = maps::find(species, from, to).ok_or_else(|| ToolError::InvalidInput {
            tool: T,
            arg: unsupported_arg(species, from),
            message: format!(
                "{species} {from} -> {to} is not available; supported: {}",
                supported()
            ),
        })?;
        let inputs = p.coordinates.unwrap_or_default();
        if inputs.is_empty() {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "coordinates",
                message: "give at least one coordinate such as 2L:1000..2000".into(),
            });
        }
        if inputs.len() > MAX_BATCH {
            return Err(ToolError::LimitExceeded {
                tool: T,
                message: format!("{MAX_BATCH} coordinates max per call; split the list"),
            });
        }
        let mut counts = [0usize; 4];
        let conversions: Vec<Value> = inputs
            .iter()
            .map(|input| match maps::parse_coordinate(input) {
                Err(msg) => {
                    counts[3] += 1;
                    json!({"input": input, "status": "invalid", "from": Value::Null, "to": Value::Null, "from_length": Value::Null, "to_length": Value::Null, "strand_flipped": false, "regions_spanned": 0, "blocks": [], "notes": [msg]})
                }
                Ok(parsed) => {
                    let c = map.range(&parsed.chromosome, parsed.start, parsed.end);
                    counts[match c.status { Status::Mapped => 0, Status::Partial => 1, Status::Unmapped => 2 }] += 1;
                    let mut v = serde_json::to_value(&c).unwrap_or_default();
                    v["input"] = json!(input);
                    v["from"] = json!({"chromosome": parsed.chromosome, "start": parsed.start, "end": parsed.end});
                    v
                }
            })
            .collect();
        let summary = format!(
            "{} of {} coordinate(s) mapped from {} release {} to release {} ({} partial, {} unmapped, {} invalid).",
            counts[0],
            inputs.len(),
            map.pair.species,
            from,
            to,
            counts[1],
            counts[2],
            counts[3]
        );
        let r = Envelope::new(T, &self.release)
            .summary(summary)
            .field("pair", json!({"species": map.pair.species, "from_release": from, "to_release": to, "source": map.source}))
            .field("conversions", json!(conversions))
            .field("counts", json!({"mapped": counts[0], "partial": counts[1], "unmapped": counts[2], "invalid": counts[3]}))
            .link("converter", CONVERTER_URL.into())
            .finish();
        Ok(note_truncation(r, inputs.len()))
    }
}

/// The argument to blame for an unsupported pair: the first of species, from_release, to_release
/// that no bundled map matches.
fn unsupported_arg(species: &str, from: u8) -> &'static str {
    let pairs = maps::pairs();
    let same_species = || {
        pairs
            .iter()
            .filter(|m| m.species.eq_ignore_ascii_case(species))
    };
    if same_species().next().is_none() {
        "species"
    } else if !same_species().any(|m| m.from_release == from) {
        "from_release"
    } else {
        "to_release"
    }
}

/// When the envelope dropped conversions to fit the size limit, say in the summary how many are shown.
/// Conversions keep input order, so the caller can resubmit the rest.
fn note_truncation(mut r: CallToolResult, requested: usize) -> CallToolResult {
    let Some(body) = r.structured_content.as_ref() else {
        return r;
    };
    if body["truncated"] != json!(true) {
        return r;
    }
    let shown = body["conversions"].as_array().map_or(0, Vec::len);
    let line = format!(
        "Showing {shown} of {requested} conversions (in input order) because the response hit the size limit; counts cover all {requested}. Resubmit from input {} in smaller batches.",
        shown + 1
    );
    if let Some(text) = r
        .content
        .first()
        .and_then(|c| c.as_text())
        .map(|t| t.text.clone())
    {
        let new = match text.split_once('\n') {
            Some((first, rest)) => format!("{first} {line}\n{rest}"),
            None => format!("{text} {line}"),
        };
        r.content = vec![rmcp::model::ContentBlock::text(new)];
    }
    r
}
