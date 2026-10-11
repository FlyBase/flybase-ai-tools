use crate::limits::MAX_JSON_BYTES;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{Map, Value, json};

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
    pub fn finish(self) -> CallToolResult {
        let mut body = Value::Object(self.data);
        body["tool"] = json!(self.tool);
        body["release"] = json!(self.release);
        body["links"] = Value::Object(self.links);
        let mut truncated = false;
        while serde_json::to_vec(&body).map(|b| b.len()).unwrap_or(0) > MAX_JSON_BYTES {
            if !halve_largest_array(&mut body) {
                break;
            }
            truncated = true;
        }
        body["truncated"] = json!(truncated);
        let mut text = self.summary;
        if truncated {
            text.push_str("\n\n(Result truncated to fit the size limit. Use limit and offset, or request fewer sections, to page through the rest.)");
        }
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
        assert!(serde_json::to_vec(&s).unwrap().len() <= MAX_JSON_BYTES + 64);
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
}
