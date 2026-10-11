use crate::error::ToolResult;
use crate::limits::MAX_JSON_BYTES;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Map, Value, json};
use std::time::Instant;

pub struct Envelope {
    tool: &'static str,
    release: String,
    summary: String,
    links: Map<String, Value>,
    data: Map<String, Value>,
}

pub fn report_link(id: &str) -> String {
    format!("https://flybase.org/reports/{id}")
}

/// Controlled-vocabulary terms (GO:0008150, FBbt:00000001) have no report page; they link to the CV report.
pub fn cv_term_link(id: &str) -> String {
    format!("https://flybase.org/cgi-bin/cvreport.pl?id={id}")
}

/// Record the per-call metrics every tool handler emits (spec section 8): a call counter by tool and
/// outcome ("ok" or the error code) and a duration histogram by tool.
pub fn record_call(tool: &'static str, outcome: &'static str, started: Instant) {
    let elapsed = started.elapsed();
    tracing::info!(
        tool,
        outcome,
        duration_ms = elapsed.as_millis() as u64,
        "tool call"
    );
    metrics::counter!("flybase_mcp_tool_calls_total", "tool" => tool, "outcome" => outcome)
        .increment(1);
    metrics::histogram!("flybase_mcp_tool_duration_seconds", "tool" => tool)
        .record(elapsed.as_secs_f64());
}

/// Turn a handler's outcome into the MCP result and record its metrics. Every tool handler ends here.
pub fn complete(
    tool: &'static str,
    started: Instant,
    res: ToolResult<CallToolResult>,
) -> CallToolResult {
    match res {
        Ok(r) => {
            record_call(tool, "ok", started);
            r
        }
        Err(e) => {
            record_call(tool, e.code(), started);
            e.into_result()
        }
    }
}

const TRUNCATION_NOTE: &str = "(Result truncated to fit the size limit. Request fewer items or sections, or page with limit/offset where the tool supports it.)";

/// Find the longest array in a JSON tree and halve it. Returns false when nothing could be halved.
/// Ties prefer the outer array, then the first child in iteration order.
fn halve_largest_array(v: &mut Value) -> bool {
    /// Length of the longest array anywhere in the tree, or None when the tree holds no array.
    fn longest(v: &Value) -> Option<usize> {
        match v {
            Value::Array(a) => Some(a.iter().filter_map(longest).fold(a.len(), usize::max)),
            Value::Object(o) => o.values().filter_map(longest).max(),
            _ => None,
        }
    }
    /// Descend to the first array whose length equals `target`.
    fn descend(v: &mut Value, target: usize) -> Option<&mut Vec<Value>> {
        match v {
            Value::Array(a) => {
                if a.len() == target {
                    return Some(a);
                }
                let i = a.iter().position(|c| longest(c) == Some(target))?;
                descend(&mut a[i], target)
            }
            Value::Object(o) => {
                let k = o
                    .iter()
                    .find(|(_, c)| longest(c) == Some(target))?
                    .0
                    .clone();
                descend(o.get_mut(&k)?, target)
            }
            _ => None,
        }
    }
    let target = match longest(v) {
        Some(n) if n > 1 => n,
        _ => return false,
    };
    match descend(v, target) {
        Some(a) => {
            a.truncate(target / 2);
            true
        }
        None => false,
    }
}

impl Envelope {
    pub fn new(tool: &'static str, release: &str) -> Self {
        Self {
            tool,
            release: release.to_string(),
            summary: String::new(),
            links: Map::new(),
            data: Map::new(),
        }
    }
    pub fn summary(mut self, s: impl Into<String>) -> Self {
        self.summary = s.into();
        self
    }
    pub fn link(mut self, name: &str, url: String) -> Self {
        self.links.insert(name.into(), Value::String(url));
        self
    }
    pub fn field(mut self, name: &str, v: Value) -> Self {
        self.data.insert(name.into(), v);
        self
    }
    /// Build the result. `structured_content` is the body; the text block is the summary, a blank line, and
    /// the same body as compact JSON, so clients that read only `content` still get the data.
    /// Arrays are halved until the body JSON fits MAX_JSON_BYTES and the whole text fits twice that.
    pub fn finish(self) -> CallToolResult {
        debug_assert!(
            !["tool", "release", "links", "truncated"]
                .iter()
                .any(|k| self.data.contains_key(*k)),
            "a data field is named like an envelope key"
        );
        let mut body = Value::Object(self.data);
        body["tool"] = json!(self.tool);
        body["release"] = json!(self.release);
        body["links"] = Value::Object(self.links);
        body["truncated"] = json!(false);
        let mut summary = self.summary;
        let mut truncated = false;
        let compact = loop {
            let json = serde_json::to_string(&body).unwrap_or_default();
            let text_len = summary.len() + 1 + TRUNCATION_NOTE.len() + 2 + json.len();
            if json.len() <= MAX_JSON_BYTES && text_len <= 2 * MAX_JSON_BYTES {
                break json;
            }
            if !halve_largest_array(&mut body) {
                break json;
            }
            truncated = true;
            body["truncated"] = json!(true);
        };
        if truncated {
            summary.push('\n');
            summary.push_str(TRUNCATION_NOTE);
        }
        let text = format!("{summary}\n\n{compact}");
        let mut r = CallToolResult::structured(body);
        r.content = vec![ContentBlock::text(text)];
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_arrays_are_truncated_and_flagged() {
        let big: Vec<Value> = (0..200_000)
            .map(|i| json!({"i": i, "s": "xxxxxxxxxx"}))
            .collect();
        let r = Envelope::new("t", "FB2026_05")
            .summary("s")
            .field("sections", json!({"alleles": {"items": big}}))
            .finish();
        let s = r.structured_content.unwrap();
        assert_eq!(s["truncated"], true);
        assert!(serde_json::to_vec(&s).unwrap().len() <= MAX_JSON_BYTES);
        assert_eq!(s["release"], "FB2026_05");
        assert!(s["sections"]["alleles"]["items"].as_array().unwrap().len() < 200_000);
    }
    #[test]
    fn small_result_untouched() {
        let r = Envelope::new("t", "r")
            .summary("ok")
            .link("report", report_link("FBgn0004647"))
            .finish();
        let s = r.structured_content.unwrap();
        assert_eq!(s["truncated"], false);
        assert_eq!(
            s["links"]["report"],
            "https://flybase.org/reports/FBgn0004647"
        );
        assert_eq!(r.content.len(), 1);
    }
    /// The text after the first blank line is the structured content, so content-only clients get the data.
    fn text_json(r: &CallToolResult) -> Value {
        let text = r.content[0].as_text().unwrap().text.clone();
        let (_, json) = text.split_once("\n\n").expect("blank line");
        serde_json::from_str(json).unwrap()
    }
    #[test]
    fn text_block_carries_summary_and_body() {
        let r = Envelope::new("t", "r")
            .summary("two lines\nof summary")
            .field("n", json!(1))
            .finish();
        let text = r.content[0].as_text().unwrap().text.clone();
        assert!(text.starts_with("two lines\nof summary\n\n{"));
        assert_eq!(text_json(&r), *r.structured_content.as_ref().unwrap());
    }
    #[test]
    fn truncated_text_block_matches_structured_content_and_fits() {
        let big: Vec<Value> = (0..200_000).map(|i| json!({"i": i})).collect();
        let r = Envelope::new("t", "r")
            .summary("s")
            .field("items", json!(big))
            .finish();
        let s = r.structured_content.clone().unwrap();
        assert_eq!(s["truncated"], true);
        assert_eq!(text_json(&r), s);
        let text = &r.content[0].as_text().unwrap().text;
        assert!(text.starts_with("s\n(Result truncated"), "{}", &text[..80]);
        assert!(text.len() <= 2 * MAX_JSON_BYTES);
        assert!(serde_json::to_vec(&s).unwrap().len() <= MAX_JSON_BYTES);
    }
    #[test]
    fn links_are_well_formed() {
        assert_eq!(
            cv_term_link("GO:0008150"),
            "https://flybase.org/cgi-bin/cvreport.pl?id=GO:0008150"
        );
    }
}
