//! search_wiki and get_wiki_page: read-only access to the FlyBase wiki (help pages and the field-by-field
//! report guides) over the private path to its MediaWiki API. Both are cached in the response cache and hold
//! `Limits::wiki`, never a chado permit. No query or title text is logged (`complete` logs no arguments;
//! the client logs errors by code alone).
use crate::backends::cache::{RESPONSE_CACHE_TTL, ResponseCache};
use crate::backends::wiki::{
    PUBLIC_WIKI, ParseResponse, SearchResponse, WikiClient, WikiError, clean_snippet,
    decode_entities, html_to_text,
};
use crate::{
    error::{ToolError, ToolResult},
    output::{Envelope, complete},
    server::FlyBaseServer,
};
use rmcp::{
    ErrorData, handler::server::wrapper::Parameters, model::CallToolResult, tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
use std::time::Instant;

/// Every canonical namespace name and alias on the wiki (MediaWiki 1.43, checked 2026-10-13).
pub const NAMESPACES: &[(&str, i32)] = &[
    ("Media", -2),
    ("Special", -1),
    ("Talk", 1),
    ("User", 2),
    ("User talk", 3),
    ("FlyBase Wiki", 4),
    ("Project", 4),
    ("FlyBase Wiki talk", 5),
    ("Project talk", 5),
    ("File", 6),
    ("Image", 6),
    ("File talk", 7),
    ("Image talk", 7),
    ("MediaWiki", 8),
    ("MediaWiki talk", 9),
    ("Template", 10),
    ("Template talk", 11),
    ("Help", 12),
    ("Help talk", 13),
    ("Category", 14),
    ("Category talk", 15),
    ("GeoJson", 420),
    ("GeoJson talk", 421),
    ("FlyBase", 1000),
    ("FlyBase talk", 1001),
    ("InteractiveFly", 1002),
    ("InteractiveFly talk", 1003),
];

/// Namespaces the tools return: main, Help, Category, FlyBase.
pub const READABLE: [i32; 4] = [0, 12, 14, 1000];

pub const DEFAULT_SEARCH_LIMIT: u32 = 10;
pub const MAX_SEARCH_LIMIT: u32 = 25;
pub const MAX_SEARCH_OFFSET: u32 = 500;
pub const MAX_QUERY_CHARS: usize = 300;
pub const MAX_TITLE_BYTES: usize = 255;
/// A wiki link may carry a query string and percent-encoding; the decoded title is still held to 255 bytes.
const MAX_LINK_BYTES: usize = 2048;
pub const DEFAULT_MAX_CHARS: u32 = 20_000;
pub const MAX_MAX_CHARS: u32 = 60_000;
/// How far back from the limit `cut_text` looks for a line break.
const CUT_WINDOW: usize = 1_000;

/// The namespace number of `title`: the part before the first `:` matched against NAMESPACES
/// (case-insensitive, `_` as space, surrounding whitespace trimmed); anything else is main (0), so
/// "Drosophila: overview" is main.
pub fn namespace_of(title: &str) -> i32 {
    let Some((prefix, _)) = title.split_once(':') else {
        return 0;
    };
    let norm = |s: &str| {
        s.replace('_', " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let p = norm(prefix);
    NAMESPACES
        .iter()
        .find(|(name, _)| norm(name) == p)
        .map(|(_, n)| *n)
        .unwrap_or(0)
}

/// `https://wiki.flybase.org/wiki/<Title>`: spaces become `_`; `? # % & + " < > [ ] { } | \ ^`, the
/// backtick, controls and non-ASCII bytes are percent-encoded; `/`, `:` and the rest stay as they are.
pub fn public_url(title: &str) -> String {
    let mut out = String::from(PUBLIC_WIKI);
    for b in title.trim().bytes() {
        match b {
            b' ' => out.push('_'),
            b'?' | b'#' | b'%' | b'&' | b'+' | b'"' | b'<' | b'>' | b'[' | b']' | b'{' | b'}'
            | b'|' | b'\\' | b'^' | b'`' => out.push_str(&format!("%{b:02X}")),
            b if !(0x20..0x7f).contains(&b) => out.push_str(&format!("%{b:02X}")),
            b => out.push(b as char),
        }
    }
    out
}

fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn bad_title(message: impl Into<String>) -> ToolError {
    ToolError::InvalidInput {
        tool: "get_wiki_page",
        arg: "title",
        message: message.into(),
    }
}

const READABLE_NAMES: &str = "main, Help, Category and FlyBase";

/// The title to request: a plain title loses a `#fragment`; a `https://wiki.flybase.org/wiki/<Title>` link
/// loses `?query` and `#fragment` and is percent-decoded; `_` becomes a space. At most 255 bytes, no
/// control characters. Only Special: and Media: are refused here; every other namespace is decided on the
/// title the wiki resolves (`get_wiki_page` checks `parse.title`).
pub fn title_from_input(raw: &str) -> Result<String, ToolError> {
    if raw.chars().any(char::is_control) {
        return Err(bad_title("the title contains a control character"));
    }
    let trimmed = raw.trim();
    let lower = trimmed.to_ascii_lowercase();
    let link = [
        "https://wiki.flybase.org/wiki/",
        "http://wiki.flybase.org/wiki/",
    ]
    .iter()
    .find(|p| lower.starts_with(*p))
    .map(|p| &trimmed[p.len()..]);
    let title = match link {
        Some(rest) => {
            if trimmed.len() > MAX_LINK_BYTES {
                return Err(bad_title(format!(
                    "the link is over {MAX_LINK_BYTES} bytes"
                )));
            }
            let path = rest.split(['?', '#']).next().unwrap_or("");
            let decoded = percent_decode(path)
                .ok_or_else(|| bad_title("the link's percent-encoding is not valid UTF-8"))?;
            if decoded.chars().any(char::is_control) {
                return Err(bad_title("the title contains a control character"));
            }
            decoded
        }
        None => {
            if trimmed.len() > MAX_TITLE_BYTES {
                return Err(bad_title(format!(
                    "wiki titles are at most {MAX_TITLE_BYTES} bytes"
                )));
            }
            trimmed.split('#').next().unwrap_or("").to_string()
        }
    };
    let title = title.replace('_', " ").trim().to_string();
    if title.is_empty() {
        return Err(bad_title(
            "give a page title such as 'FlyBase:Aberration Report' or a wiki.flybase.org/wiki/ link",
        ));
    }
    if title.len() > MAX_TITLE_BYTES {
        return Err(bad_title(format!(
            "wiki titles are at most {MAX_TITLE_BYTES} bytes"
        )));
    }
    if matches!(namespace_of(&title), -1 | -2) {
        return Err(bad_title(format!(
            "Special: and Media: pages cannot be read; this tool reads {READABLE_NAMES} pages"
        )));
    }
    Ok(title)
}

/// Cut `text` to at most `max` characters: at the last line break before the limit when one falls in the
/// last 1,000 characters, else at a character boundary. Returns the text and whether it was cut.
pub fn cut_text(text: &str, max: usize) -> (String, bool) {
    let Some((end, _)) = text.char_indices().nth(max) else {
        return (text.to_string(), false);
    };
    let head = &text[..end];
    let window_start = head
        .char_indices()
        .rev()
        .nth(CUT_WINDOW.saturating_sub(1))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let cut = match head.rfind('\n') {
        Some(nl) if nl >= window_start => nl,
        _ => end,
    };
    (head[..cut].trim_end().to_string(), true)
}

/// 45587 -> "45,587".
fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out
}

/// The wiki client plus the cached licence lookup. One per server, shared by every request.
pub struct Wiki {
    pub client: WikiClient,
    /// (fetched at, licence) from `siteinfo` `rightsinfo`, refreshed after the response cache TTL. A
    /// failed lookup is not stored.
    licence: tokio::sync::Mutex<Option<(Instant, Option<Value>)>>,
}

impl Wiki {
    pub fn new(client: WikiClient) -> Self {
        Self {
            client,
            licence: tokio::sync::Mutex::new(None),
        }
    }

    /// `{text, url}` when the wiki declares rights; None when both are empty or the lookup failed.
    pub async fn licence(&self) -> Option<Value> {
        let mut slot = self.licence.lock().await;
        if let Some((at, l)) = slot.as_ref()
            && at.elapsed() < RESPONSE_CACHE_TTL
        {
            return l.clone();
        }
        let r = self.client.rights().await.ok()?;
        let l = (!r.text.trim().is_empty() || !r.url.trim().is_empty())
            .then(|| json!({"text": r.text, "url": r.url}));
        *slot = Some((Instant::now(), l.clone()));
        l
    }
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SearchWikiParams {
    /// Words to search for, up to 300 characters (for example "balancer" or "batch download").
    pub query: String,
    /// Hits to return (default 10, max 25).
    pub limit: Option<u32>,
    /// Hits to skip, for paging (default 0, max 500).
    pub offset: Option<u32>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GetWikiPageParams {
    /// Page title such as "FlyBase:Aberration Report", or a https://wiki.flybase.org/wiki/... link.
    pub title: String,
    /// Characters of text to return (default 20000, max 60000).
    pub max_chars: Option<u32>,
}

fn unconfigured(tool: &'static str) -> ToolError {
    ToolError::BackendUnavailable {
        tool,
        message: format!("the wiki is not configured on this server; read it at {PUBLIC_WIKI}"),
    }
}

fn bad_query(message: impl Into<String>) -> ToolError {
    ToolError::InvalidInput {
        tool: "search_wiki",
        arg: "query",
        message: message.into(),
    }
}

/// Map hits through the redirects, drop missing, invalid, unreadable and repeated targets, keep search order.
fn hits_of(r: &SearchResponse) -> Vec<Value> {
    let mut seen = HashSet::new();
    let mut hits = Vec::new();
    for h in &r.search {
        let resolved = r
            .redirects
            .iter()
            .find(|x| x.from == h.title)
            .map(|x| x.to.as_str())
            .unwrap_or(h.title.as_str());
        let page = r.pages.iter().find(|p| p.title == resolved);
        if let Some(p) = page
            && (p.missing || p.invalid || !READABLE.contains(&p.ns))
        {
            continue;
        }
        if !seen.insert(resolved.to_string()) {
            continue;
        }
        let last_modified = page
            .and_then(|p| p.touched.clone())
            .or_else(|| (!h.timestamp.is_empty()).then(|| h.timestamp.clone()));
        let mut hit = json!({
            "title": resolved,
            "snippet": clean_snippet(&h.snippet),
            "url": public_url(resolved),
        });
        if let Some(t) = last_modified {
            hit["last_modified"] = json!(t);
        }
        hits.push(hit);
    }
    hits
}

/// "FTP archive" -> "ftp* archive*".
fn wildcard_query(q: &str) -> String {
    q.split_whitespace()
        .map(|t| format!("{}*", t.to_lowercase()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_broken_redirect(p: &ParseResponse, text: &str) -> bool {
    p.text.contains("class=\"redirectMsg\"")
        || (namespace_of(&p.title) == 0
            && (p.text.contains("<li>REDIRECT [[")
                || text.starts_with("REDIRECT [[")
                || text.starts_with("- REDIRECT [[")))
}

#[tool_router(router = tool_router_wiki, vis = "pub")]
impl FlyBaseServer {
    #[tool(
        name = "search_wiki",
        description = "Search the FlyBase wiki: help pages and field-by-field guides to FlyBase reports, tools, and downloads. Searches the wiki's text index and titles; recall is uneven (inflected words can miss), so try other words or fewer of them when nothing matches. Use when you need to know what a report field means or how a FlyBase tool works; use `search` and `get_report` for genes, alleles, and other data. Returns titles to pass to get_wiki_page."
    )]
    pub async fn search_wiki(
        &self,
        Parameters(p): Parameters<SearchWikiParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "search_wiki";
        let started = Instant::now();
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, self.search_wiki_impl(p))
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn search_wiki_impl(&self, p: SearchWikiParams) -> ToolResult<CallToolResult> {
        const T: &str = "search_wiki";
        let query = p.query.trim();
        if query.is_empty() {
            return Err(bad_query("give one or more words to search for"));
        }
        if query.chars().count() > MAX_QUERY_CHARS {
            return Err(bad_query(format!("at most {MAX_QUERY_CHARS} characters")));
        }
        if query.chars().any(char::is_control) {
            return Err(bad_query("the query contains a control character"));
        }
        let limit = p.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
        if !(1..=MAX_SEARCH_LIMIT).contains(&limit) {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "limit",
                message: format!("1 to {MAX_SEARCH_LIMIT}"),
            });
        }
        let offset = p.offset.unwrap_or(0);
        if offset > MAX_SEARCH_OFFSET {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "offset",
                message: format!("at most {MAX_SEARCH_OFFSET}"),
            });
        }
        let wiki = self.wiki.as_ref().ok_or_else(|| unconfigured(T))?;
        let _permit = self.limits.wiki(T).await?;
        let mut r = wiki
            .client
            .search(query, limit, offset)
            .await
            .map_err(|e| e.into_tool(T))?;
        let mut wildcards = false;
        if r.search.is_empty() && offset == 0 && !query.contains(['*', '"']) {
            let retry = wiki
                .client
                .search(&wildcard_query(query), limit, offset)
                .await
                .map_err(|e| e.into_tool(T))?;
            wildcards = !retry.search.is_empty();
            r = retry;
        }
        let hits = hits_of(&r);
        let total = r.searchinfo.totalhits;
        let has_more = u64::from(offset) + (r.search.len() as u64) < total;
        let mut summary = match hits.len() {
            0 => format!(
                "No wiki pages match '{query}'. Try other or fewer words; the wiki's search misses some inflections."
            ),
            1 => format!(
                "1 wiki page matches '{query}'. Call get_wiki_page with its title to read it."
            ),
            n => format!(
                "{n} wiki pages match '{query}'. Call get_wiki_page with a title to read one."
            ),
        };
        if wildcards {
            summary.push_str(" (Matched only with a wildcard on each word.)");
        }
        if has_more {
            summary.push_str(&format!(
                " More results: pass offset={}.",
                u64::from(offset) + r.search.len() as u64
            ));
        }
        Ok(Envelope::new(T, &self.release)
            .summary(summary)
            .field("hits", json!(hits))
            .field("total", json!(total))
            .field("has_more", json!(has_more))
            .field("matched_with_wildcards", json!(wildcards))
            .finish())
    }

    #[tool(
        name = "get_wiki_page",
        description = "Read a FlyBase wiki page as plain text (markup removed), with its section list. Use after `search_wiki`, or with a title such as 'FlyBase:Aberration Report' or a https://wiki.flybase.org/wiki/ link. Long pages are cut at max_chars (default 20000, up to 60000) and say so in text_truncated. Reads main, Help, Category and FlyBase pages; Category pages return only their description, not their members."
    )]
    pub async fn get_wiki_page(
        &self,
        Parameters(p): Parameters<GetWikiPageParams>,
    ) -> Result<CallToolResult, ErrorData> {
        const T: &str = "get_wiki_page";
        let started = Instant::now();
        let res = async {
            let key = ResponseCache::key(T, &p);
            self.caches
                .response
                .get_or_else(key, self.get_wiki_page_impl(p))
                .await
        }
        .await;
        Ok(complete(T, started, res))
    }

    async fn get_wiki_page_impl(&self, p: GetWikiPageParams) -> ToolResult<CallToolResult> {
        const T: &str = "get_wiki_page";
        let title = title_from_input(&p.title)?;
        let max_chars = p.max_chars.unwrap_or(DEFAULT_MAX_CHARS);
        if !(1..=MAX_MAX_CHARS).contains(&max_chars) {
            return Err(ToolError::InvalidInput {
                tool: T,
                arg: "max_chars",
                message: format!("1 to {MAX_MAX_CHARS}"),
            });
        }
        let wiki = self.wiki.as_ref().ok_or_else(|| unconfigured(T))?;
        let _permit = self.limits.wiki(T).await?;
        let (page, licence) = tokio::join!(wiki.client.page(&title), wiki.licence());
        let page = page.map_err(|e| match e {
            WikiError::NotFound => ToolError::NotFound {
                tool: T,
                message: format!("no wiki page titled '{title}'; try search_wiki"),
            },
            WikiError::TooLarge => ToolError::LimitExceeded {
                tool: T,
                message: format!(
                    "this wiki page is too large to return; read it at {}",
                    public_url(&title)
                ),
            },
            e => e.into_tool(T),
        })?;
        // The decision runs on the title the wiki resolved, after any redirect.
        if !READABLE.contains(&namespace_of(&page.title)) {
            return Err(bad_title(format!(
                "'{}' is not in a namespace this tool reads ({READABLE_NAMES})",
                page.title
            )));
        }
        let text: String = html_to_text(&page.text)
            .chars()
            .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
            .collect();
        if is_broken_redirect(&page, text.trim_start()) {
            return Err(ToolError::NotFound {
                tool: T,
                message: format!(
                    "'{title}' is a redirect to a page that does not exist; try search_wiki"
                ),
            });
        }
        let url = public_url(&page.title);
        let total_chars = text.chars().count();
        let (text, text_truncated) = cut_text(&text, max_chars as usize);
        let sections: Vec<Value> = page
            .sections
            .iter()
            .map(|s| {
                json!({
                    "level": s.level.trim().parse::<u8>().unwrap_or(1),
                    "title": decode_entities(&strip_tags(&s.line)).trim(),
                })
            })
            .collect();
        let summary = if text_truncated {
            format!(
                "{}: showing {} of {} characters; pass max_chars up to {} or read it at {url}.",
                page.title,
                thousands(text.chars().count()),
                thousands(total_chars),
                thousands(MAX_MAX_CHARS as usize),
            )
        } else {
            format!(
                "{}: {} characters, {} sections. Cite it as {url}.",
                page.title,
                thousands(total_chars),
                sections.len()
            )
        };
        let mut env = Envelope::new(T, &self.release)
            .summary(summary)
            .link("wiki", url.clone())
            .field("title", json!(page.title))
            .field("url", json!(url))
            .field("text", json!(text))
            .field("text_truncated", json!(text_truncated))
            .field("total_chars", json!(total_chars))
            .field("sections", json!(sections))
            .field("revision", json!(page.revid));
        if let Some(l) = licence {
            env = env.field("licence", l);
        }
        Ok(env.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespaces_match_case_and_underscores() {
        assert_eq!(namespace_of("user:Spammer"), 2);
        assert_eq!(namespace_of(" User:Spammer"), 2);
        assert_eq!(namespace_of("User_talk:Spammer"), 3);
        assert_eq!(namespace_of("MediaWiki:Sidebar"), 8);
        assert_eq!(namespace_of("Image:X.png"), 6);
        assert_eq!(namespace_of("GeoJson:X"), 420);
        assert_eq!(namespace_of("Project:About"), 4);
        assert_eq!(namespace_of("FlyBase Wiki:About"), 4);
        assert_eq!(namespace_of("help:Contents"), 12);
        assert_eq!(
            namespace_of("FlyBase:Balancer/Genotype variant Report"),
            1000
        );
        assert_eq!(namespace_of("Drosophila: overview"), 0);
        assert_eq!(namespace_of("FBgn0000553"), 0);
        assert_eq!(namespace_of("special:RecentChanges"), -1);
        for ns in [0, 12, 14, 1000] {
            assert!(READABLE.contains(&ns));
        }
    }

    #[test]
    fn public_urls_follow_the_encode_set() {
        assert_eq!(
            public_url("FlyBase:Balancer/Genotype variant Report"),
            "https://wiki.flybase.org/wiki/FlyBase:Balancer/Genotype_variant_Report"
        );
        assert_eq!(
            public_url("FlyBase:New to Flies?"),
            "https://wiki.flybase.org/wiki/FlyBase:New_to_Flies%3F"
        );
        assert_eq!(
            public_url("Help:Dmel\\vn"),
            "https://wiki.flybase.org/wiki/Help:Dmel%5Cvn"
        );
        assert_eq!(
            public_url("A#b%c"),
            "https://wiki.flybase.org/wiki/A%23b%25c"
        );
        assert_eq!(public_url("Ré"), "https://wiki.flybase.org/wiki/R%C3%A9");
        assert_eq!(
            public_url("A (b), c's! ~@$*;"),
            "https://wiki.flybase.org/wiki/A_(b),_c's!_~@$*;"
        );
    }

    #[test]
    fn titles_from_links_and_plain_input() {
        assert_eq!(
            title_from_input(
                "https://wiki.flybase.org/wiki/FlyBase:Aberration_Report?action=history#x"
            )
            .unwrap(),
            "FlyBase:Aberration Report"
        );
        assert_eq!(
            title_from_input("https://wiki.flybase.org/wiki/%55ser:Spammer").unwrap(),
            "User:Spammer"
        );
        assert_eq!(
            title_from_input("FlyBase:About#Warranty").unwrap(),
            "FlyBase:About"
        );
        assert_eq!(
            title_from_input(" User_talk:Spammer ").unwrap(),
            "User talk:Spammer"
        );
        assert_eq!(
            title_from_input("Drosophila: overview").unwrap(),
            "Drosophila: overview"
        );
        for bad in [
            "Special:RecentChanges",
            "media:X.png",
            "",
            "   ",
            "#frag",
            "a\nb",
            "https://wiki.flybase.org/wiki/A%0Ab",
            "https://wiki.flybase.org/wiki/%FF",
        ] {
            let e = title_from_input(bad).unwrap_err();
            assert_eq!(e.code(), "invalid_input", "{bad:?}");
        }
        assert!(title_from_input(&"a".repeat(255)).is_ok());
        assert!(title_from_input(&"a".repeat(256)).is_err());
        // 85 three-byte characters are 255 bytes; one more is 258.
        assert!(title_from_input(&"€".repeat(85)).is_ok());
        assert!(title_from_input(&"€".repeat(86)).is_err());
    }

    #[test]
    fn cut_prefers_a_line_break() {
        let text = format!("{}\n{}", "a".repeat(50), "b".repeat(100));
        let (t, cut) = cut_text(&text, 120);
        assert!(cut);
        assert_eq!(t, "a".repeat(50));
        let (t, cut) = cut_text(&text, 1000);
        assert!(!cut);
        assert_eq!(t, text);
        // No line break within the last 1,000 characters: cut at the limit.
        let long = format!("x\n{}", "y".repeat(5000));
        let (t, cut) = cut_text(&long, 3000);
        assert!(cut);
        assert_eq!(t.chars().count(), 3000);
    }

    #[test]
    fn cut_lands_on_a_char_boundary() {
        let text = "é".repeat(30);
        let (t, cut) = cut_text(&text, 7);
        assert!(cut);
        assert_eq!(t, "é".repeat(7));
        let mixed = "aé🦀".repeat(10);
        let (t, _) = cut_text(&mixed, 5);
        assert_eq!(t, "aé🦀aé");
    }

    #[test]
    fn wildcard_terms() {
        assert_eq!(wildcard_query("FTP archive"), "ftp* archive*");
        assert_eq!(wildcard_query("  aberr  "), "aberr*");
    }

    #[test]
    fn thousands_separators() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(20000), "20,000");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
