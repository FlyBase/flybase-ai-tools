//! Read-only client for the FlyBase wiki's MediaWiki API (`api.php`), plus the converters that turn search
//! snippets (wikitext) and parsed pages (HTML) into plain text. Three GET requests only; the client cannot
//! write (writes need POST and a token it never fetches). No query or title text ever reaches a log: every
//! reqwest error is rendered with `without_url()`, and MediaWiki errors are logged by code alone.
use crate::error::ToolError;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::time::Duration;
use url::Url;

/// Largest API response the client reads (the biggest page today is about 1.03 MB of parse JSON).
pub const MAX_WIKI_BYTES: usize = 4 * 1024 * 1024;
/// Prefix of every public wiki link. Never derived from the private base URL.
pub const PUBLIC_WIKI: &str = "https://wiki.flybase.org/wiki/";
/// Namespaces `search` covers: Help, Category, FlyBase. The main namespace is mostly redirect stubs.
pub const SEARCH_NAMESPACES: &str = "12|14|1000";

const TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct SearchInfo {
    #[serde(default)]
    pub totalhits: u64,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct SearchHit {
    #[serde(default)]
    pub ns: i32,
    pub title: String,
    #[serde(default)]
    pub snippet: String,
    #[serde(default)]
    pub timestamp: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Redirect {
    pub from: String,
    pub to: String,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct PageInfo {
    /// Absent on an invalid title.
    #[serde(default)]
    pub ns: i32,
    pub title: String,
    #[serde(default)]
    pub missing: bool,
    #[serde(default)]
    pub invalid: bool,
    #[serde(default)]
    pub touched: Option<String>,
}

/// `query` of the combined `list=search` + `generator=search&redirects=1&prop=info` request.
#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct SearchResponse {
    #[serde(default)]
    pub searchinfo: SearchInfo,
    #[serde(default)]
    pub search: Vec<SearchHit>,
    #[serde(default)]
    pub redirects: Vec<Redirect>,
    #[serde(default)]
    pub pages: Vec<PageInfo>,
}

#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct Section {
    /// MediaWiki sends the level as a string (`"2"`).
    pub level: String,
    /// HTML-escaped heading text (`Gene Model &amp; Products`).
    pub line: String,
    #[serde(default)]
    pub anchor: String,
}

/// `parse` of `action=parse&prop=text|sections|revid` (formatversion 2).
#[derive(Deserialize, Debug, Clone, PartialEq)]
pub struct ParseResponse {
    /// Normalized, post-redirect title.
    pub title: String,
    #[serde(default)]
    pub pageid: u64,
    #[serde(default)]
    pub revid: u64,
    pub text: String,
    #[serde(default)]
    pub sections: Vec<Section>,
    #[serde(default)]
    pub redirects: Vec<Redirect>,
}

#[derive(Deserialize, Debug, Clone, Default, PartialEq)]
pub struct RightsInfo {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WikiError {
    /// `missingtitle` or `pagecannotexist`.
    NotFound,
    /// Any other MediaWiki error code (`invalidtitle`, `badvalue`, ...): the request itself is wrong.
    Invalid(String),
    /// The response is over MAX_WIKI_BYTES.
    TooLarge,
    /// Transport error, timeout, redirect, non-200, non-JSON, unexpected shape, or an outage code.
    Unavailable(String),
}

impl WikiError {
    pub fn into_tool(self, tool: &'static str) -> ToolError {
        match self {
            Self::NotFound => ToolError::NotFound {
                tool,
                message: "no wiki page has that title; try search_wiki".into(),
            },
            Self::Invalid(code) => {
                tracing::info!(tool, code = %code, "wiki rejected the request");
                ToolError::InvalidInput {
                    tool,
                    arg: if tool == "search_wiki" {
                        "query"
                    } else {
                        "title"
                    },
                    message: format!("the wiki rejected it ({code})"),
                }
            }
            Self::TooLarge => ToolError::LimitExceeded {
                tool,
                message: "this wiki page is too large to return; read it on the wiki".into(),
            },
            Self::Unavailable(m) => {
                tracing::error!(tool, error = %m, "wiki unavailable");
                ToolError::BackendUnavailable {
                    tool,
                    message: "the FlyBase wiki is not reachable; try again in a few minutes".into(),
                }
            }
        }
    }
}

/// A MediaWiki error code reduced to identifier characters, so nothing else the server sent is echoed.
fn sanitize_code(code: &str) -> String {
    let c: String = code
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect();
    if c.is_empty() { "unknown".into() } else { c }
}

fn map_code(code: &str) -> WikiError {
    let code = sanitize_code(code);
    match code.as_str() {
        "missingtitle" | "pagecannotexist" => WikiError::NotFound,
        "readonly" | "maxlag" | "ratelimited" => {
            WikiError::Unavailable(format!("MediaWiki error {code}"))
        }
        c if c.starts_with("internal_api_error") => {
            WikiError::Unavailable(format!("MediaWiki error {code}"))
        }
        _ => WikiError::Invalid(code),
    }
}

fn transport(e: reqwest::Error) -> WikiError {
    WikiError::Unavailable(e.without_url().to_string())
}

pub struct WikiClient {
    http: reqwest::Client,
    api: Url,
    host: reqwest::header::HeaderValue,
}

impl WikiClient {
    /// `base` is the wiki root (`http://<wiki-private-ip>/`); requests go to `<base>api.php`. Only
    /// `http://` is accepted: this build has no TLS backend. `host_header` is sent as `Host` on every
    /// request so the origin answers as `wiki.flybase.org`.
    pub fn new(mut base: Url, host_header: String) -> anyhow::Result<Self> {
        if base.scheme() != "http" {
            anyhow::bail!("WIKI_BASE_URL must be http://; this build has no TLS backend");
        }
        if !base.path().ends_with('/') {
            let p = format!("{}/", base.path());
            base.set_path(&p);
        }
        base.set_query(None);
        base.set_fragment(None);
        let api = base.join("api.php")?;
        let host = reqwest::header::HeaderValue::from_str(host_header.trim())
            .map_err(|_| anyhow::anyhow!("WIKI_HOST_HEADER is not a valid header value"))?;
        if host.is_empty() {
            anyhow::bail!("WIKI_HOST_HEADER is empty");
        }
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            // The origin serves the wiki directly; a redirect means something is wrong, and following one
            // could fetch an arbitrary reachable URL.
            .redirect(reqwest::redirect::Policy::none())
            .user_agent(concat!("flybase-mcp/", env!("CARGO_PKG_VERSION")))
            .build()?;
        Ok(Self { http, api, host })
    }

    /// `api.php` with `format=json&formatversion=2` and then `pairs`, every pair added through
    /// `append_pair` so user text is always one percent-encoded value.
    fn url(&self, pairs: &[(&str, &str)]) -> Url {
        let mut u = self.api.clone();
        {
            let mut q = u.query_pairs_mut();
            for (k, v) in pairs {
                q.append_pair(k, v);
            }
            q.append_pair("format", "json");
            q.append_pair("formatversion", "2");
        }
        u
    }

    /// GET `url`, read at most MAX_WIKI_BYTES, decode the JSON, map a MediaWiki `error`, and deserialize
    /// the `key` member.
    async fn get<T: DeserializeOwned>(
        &self,
        url: Url,
        key: &str,
        timeout: Option<Duration>,
    ) -> Result<T, WikiError> {
        let mut rb = self
            .http
            .get(url)
            .header(reqwest::header::HOST, self.host.clone());
        if let Some(t) = timeout {
            rb = rb.timeout(t);
        }
        let mut resp = rb.send().await.map_err(transport)?;
        let status = resp.status().as_u16();
        if (300..400).contains(&status) {
            return Err(WikiError::Unavailable(format!(
                "unexpected redirect (HTTP {status})"
            )));
        }
        if status != 200 {
            return Err(WikiError::Unavailable(format!("HTTP {status}")));
        }
        if let Some(n) = resp.content_length()
            && n > MAX_WIKI_BYTES as u64
        {
            tracing::warn!(bytes = n, "wiki response over the 4 MiB cap");
            return Err(WikiError::TooLarge);
        }
        let mut buf: Vec<u8> = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(transport)? {
            if buf.len() + chunk.len() > MAX_WIKI_BYTES {
                tracing::warn!(
                    bytes = buf.len() + chunk.len(),
                    "wiki response over the 4 MiB cap"
                );
                return Err(WikiError::TooLarge);
            }
            buf.extend_from_slice(&chunk);
        }
        let mut v: serde_json::Value = serde_json::from_slice(&buf)
            .map_err(|_| WikiError::Unavailable("non-JSON response".into()))?;
        if let Some(err) = v.get("error") {
            let code = err.get("code").and_then(|c| c.as_str()).unwrap_or("");
            return Err(map_code(code));
        }
        let member = v
            .get_mut(key)
            .map(serde_json::Value::take)
            .unwrap_or(serde_json::Value::Null);
        // The serde message can quote response values, so it is not kept.
        serde_json::from_value(member)
            .map_err(|_| WikiError::Unavailable(format!("unexpected response shape ({key})")))
    }

    /// One combined request: `list=search` for titles, snippets and timestamps, plus
    /// `generator=search&redirects=1&prop=info` for the redirect map and the resolved pages.
    pub async fn search(
        &self,
        query: &str,
        limit: u32,
        offset: u32,
    ) -> Result<SearchResponse, WikiError> {
        let limit = limit.to_string();
        let offset = offset.to_string();
        let url = self.url(&[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", query),
            ("srnamespace", SEARCH_NAMESPACES),
            ("srlimit", &limit),
            ("sroffset", &offset),
            ("srprop", "snippet|timestamp"),
            ("srinfo", "totalhits"),
            ("generator", "search"),
            ("gsrsearch", query),
            ("gsrnamespace", SEARCH_NAMESPACES),
            ("gsrlimit", &limit),
            ("gsroffset", &offset),
            ("redirects", "1"),
            ("prop", "info"),
        ]);
        // MediaWiki omits `query` entirely in some empty cases; that is an empty result.
        let q: Option<SearchResponse> = self.get(url, "query", None).await?;
        Ok(q.unwrap_or_default())
    }

    /// The parsed page (following redirects), without edit links, TOC, or the limit report.
    pub async fn page(&self, title: &str) -> Result<ParseResponse, WikiError> {
        let url = self.url(&[
            ("action", "parse"),
            ("page", title),
            ("redirects", "1"),
            ("prop", "text|sections|revid"),
            ("disablelimitreport", "1"),
            ("disableeditsection", "1"),
            ("disabletoc", "1"),
        ]);
        self.get(url, "parse", None).await
    }

    fn siteinfo_url(&self) -> Url {
        self.url(&[
            ("action", "query"),
            ("meta", "siteinfo"),
            ("siprop", "rightsinfo"),
        ])
    }

    /// The wiki's licence (`$wgRightsText`, `$wgRightsUrl`); both empty when unset.
    pub async fn rights(&self) -> Result<RightsInfo, WikiError> {
        #[derive(Deserialize)]
        struct Q {
            #[serde(default)]
            rightsinfo: RightsInfo,
        }
        let q: Q = self.get(self.siteinfo_url(), "query", None).await?;
        Ok(q.rightsinfo)
    }

    /// True when the siteinfo request answers with valid JSON within 2 s.
    pub async fn healthy(&self) -> bool {
        #[derive(Deserialize)]
        struct Q {}
        self.get::<Q>(self.siteinfo_url(), "query", Some(HEALTH_TIMEOUT))
            .await
            .is_ok()
    }
}

// ---------------------------------------------------------------------------------------------------------
// Entities

/// Decode HTML entities once, left to right: `amp lt gt quot apos nbsp`, decimal and hex numeric references.
/// `&nbsp;` and `&#160;` become a plain space. Unknown or malformed references are kept as written, so
/// `&amp;lt;` gives `&lt;`.
pub fn decode_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        // An entity is at most 10 bytes before its ';' (`&#x10FFFF;`).
        let end = rest.as_bytes().iter().take(12).position(|&b| b == b';');
        let decoded = end.and_then(|e| decode_one(&rest[1..e]).map(|c| (c, e)));
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn decode_one(name: &str) -> Option<char> {
    let c = match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        _ => {
            let num = name.strip_prefix('#')?;
            let n = match num.strip_prefix(['x', 'X']) {
                Some(h) if !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit()) => {
                    u32::from_str_radix(h, 16).ok()?
                }
                Some(_) => return None,
                None if !num.is_empty() && num.bytes().all(|b| b.is_ascii_digit()) => {
                    num.parse().ok()?
                }
                None => return None,
            };
            match n {
                160 => ' ',
                // Control characters other than tab, newline and carriage return stay as written.
                0..=8 | 11 | 12 | 14..=31 | 127..=159 => return None,
                _ => char::from_u32(n)?,
            }
        }
    };
    Some(c)
}

// ---------------------------------------------------------------------------------------------------------
// Search snippets (wikitext)

/// Remove every `<...>` tag (the `searchmatch` spans and anything else).
fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match (in_tag, c) {
            (false, '<') => in_tag = true,
            (true, '>') => in_tag = false,
            (false, c) => out.push(c),
            (true, _) => {}
        }
    }
    out
}

/// `[[target|label]]` to `label`, `[[target]]` to `target`.
fn replace_internal_links(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find("[[") {
        out.push_str(&rest[..i]);
        let after = &rest[i + 2..];
        match after.find("]]") {
            Some(j) => {
                let inner = &after[..j];
                // A nested `[[` means the outer link's close is further on; keep the outer marker as text.
                if inner.contains("[[") {
                    out.push_str("[[");
                    rest = after;
                    continue;
                }
                out.push_str(inner.rsplit('|').next().unwrap_or(inner).trim());
                rest = &after[j + 2..];
            }
            None => {
                // A link cut off at the snippet's edge: keep its target text.
                out.push_str(after.split('|').next_back().unwrap_or(after));
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// `[http://x label]` to `label`, `[http://x]` to `http://x`. Brackets without a URL scheme stay.
fn replace_external_links(s: &str) -> String {
    const SCHEMES: [&str; 5] = ["http://", "https://", "ftp://", "//", "mailto:"];
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('[') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let close = after.find(']');
        match close {
            Some(j) if SCHEMES.iter().any(|p| after.starts_with(p)) => {
                let inner = &after[..j];
                match inner.split_once(char::is_whitespace) {
                    Some((_, label)) if !label.trim().is_empty() => out.push_str(label.trim()),
                    _ => out.push_str(inner.trim()),
                }
                rest = &after[j + 1..];
            }
            _ => {
                out.push('[');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Drop `{{...}}` innermost first, then any unmatched `{{` or `}}` left by a template cut at the edge.
fn drop_templates(s: &str) -> String {
    let mut t = s.to_string();
    while let Some(close) = t.find("}}") {
        match t[..close].rfind("{{") {
            Some(open) => t.replace_range(open..close + 2, ""),
            None => t.replace_range(close..close + 2, ""),
        }
    }
    t.replace("{{", "")
}

/// Strip one snippet line's leading block markup and its table cell separators.
fn clean_line(line: &str) -> String {
    let mut l = line.trim();
    loop {
        let before = l;
        for p in ["#REDIRECT", "#redirect", "#Redirect"] {
            if let Some(r) = l.strip_prefix(p) {
                l = r.trim_start();
            }
        }
        for p in ["{|", "|}", "|-", "||", "|", "!", "*", "#", ":", ";"] {
            if let Some(r) = l.strip_prefix(p) {
                l = r.trim_start();
            }
        }
        if l == before {
            break;
        }
    }
    // `== Heading ==`
    let l = l.trim_start_matches('=').trim_end_matches('=');
    let l = l.replace("||", " | ").replace("!!", " | ");
    l.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A search snippet (raw wikitext lines with `searchmatch` spans and escaped entities) as one line of plain
/// text: tags removed, entities decoded once, links reduced to their labels, templates, bold and italic
/// quotes, table and list markup, heading markers and `#REDIRECT` dropped, lines joined with one space.
pub fn clean_snippet(s: &str) -> String {
    let t = decode_entities(&strip_tags(s));
    let t = replace_internal_links(&t);
    let t = replace_external_links(&t);
    let t = drop_templates(&t);
    let t = t.replace("'''", "").replace("''", "");
    t.lines()
        .map(clean_line)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

// ---------------------------------------------------------------------------------------------------------
// Parsed pages (HTML)

const BLOCK: [&str; 16] = [
    "p",
    "div",
    "dl",
    "dt",
    "dd",
    "ul",
    "ol",
    "li",
    "table",
    "tr",
    "pre",
    "blockquote",
    "caption",
    "center",
    "section",
    "hr",
];
/// Deepest list or `<dd>` nesting that adds indentation; deeper levels indent no further, so output
/// stays linear in the input however deep the nesting.
const MAX_INDENT_DEPTH: usize = 8;
/// Longest tag the scanner reads; a `<` with no `>` within this many bytes is text.
const MAX_TAG_BYTES: usize = 4096;
/// Nested external links tracked for `text (href)`; deeper ones are plain text.
const MAX_LINK_DEPTH: usize = 16;

const VOID: [&str; 12] = [
    "br", "hr", "img", "meta", "link", "input", "wbr", "col", "source", "area", "base", "track",
];

struct Tag<'a> {
    name: String,
    closing: bool,
    self_closing: bool,
    attrs: &'a str,
}

/// The value of attribute `want` in a raw attribute string (quoted or not), names matched case-insensitively.
fn attr(attrs: &str, want: &str) -> Option<String> {
    let b = attrs.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let ns = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        let name = &attrs[ns..i];
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = "";
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                let vs = i + 1;
                i = vs;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = &attrs[vs..i];
                i += 1;
            } else {
                let vs = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                value = &attrs[vs..i];
            }
        }
        if name.eq_ignore_ascii_case(want) {
            return Some(value.to_string());
        }
        if name.is_empty() && i < b.len() {
            i += 1;
        }
    }
    None
}

fn has_class(attrs: &str, class: &str) -> bool {
    attr(attrs, "class").is_some_and(|c| c.split_whitespace().any(|x| x == class))
}

/// Plain-text builder: collapses whitespace outside `<pre>`, at most one blank line in a row, and turns line
/// breaks inside a table cell into spaces.
#[derive(Default)]
struct Out {
    buf: String,
    space: bool,
    pre: usize,
    cell: usize,
    cells_in_row: usize,
    list: usize,
    dd: usize,
    links: Vec<(Option<String>, usize)>,
    /// `<a>` elements opened beyond MAX_LINK_DEPTH.
    untracked_links: usize,
}

impl Out {
    fn at_break(&self) -> bool {
        self.buf.is_empty() || self.buf.ends_with(['\n', '\t', ' '])
    }

    fn trim_end_spaces(&mut self) {
        while self.buf.ends_with(' ') {
            self.buf.pop();
        }
    }

    fn text(&mut self, raw: &str) {
        let t = decode_entities(raw);
        if self.pre > 0 {
            self.buf.push_str(&t);
            self.space = false;
            return;
        }
        for c in t.chars() {
            if c.is_whitespace() {
                self.space = true;
            } else {
                if self.space && !self.at_break() {
                    self.buf.push(' ');
                }
                self.space = false;
                self.buf.push(c);
            }
        }
    }

    fn push_raw(&mut self, s: &str) {
        if self.space && !self.at_break() {
            self.buf.push(' ');
        }
        self.space = false;
        self.buf.push_str(s);
    }

    /// End the current line (a space inside a table cell). Several breaks in a row give one line end.
    fn newline(&mut self) {
        if self.cell > 0 {
            self.space = true;
            return;
        }
        self.space = false;
        if self.pre == 0 {
            self.trim_end_spaces();
        }
        if self.buf.is_empty() || self.buf.ends_with('\n') {
            return;
        }
        self.buf.push('\n');
    }

    fn blank_line(&mut self) {
        self.newline();
        if self.cell == 0 && !self.buf.is_empty() && !self.buf.ends_with("\n\n") {
            self.buf.push('\n');
        }
    }

    fn finish(mut self) -> String {
        self.trim_end_spaces();
        let lines: Vec<&str> = self.buf.lines().collect();
        let mut out = String::with_capacity(self.buf.len());
        let mut blank = 0;
        for l in lines {
            if l.trim().is_empty() {
                blank += 1;
                if blank > 1 {
                    continue;
                }
                out.push('\n');
            } else {
                blank = 0;
                out.push_str(l);
                out.push('\n');
            }
        }
        out.trim_matches('\n').to_string()
    }
}

/// Parse a tag starting at `s[0] == '<'`. Returns the tag and its byte length, or None when `<` does not
/// begin a tag. A `>` inside a quoted attribute value does not end the tag. The scan is bounded: an
/// unquoted `<` or MAX_TAG_BYTES without a `>` means this `<` is text, so unclosed tags cost linear time.
fn parse_tag(s: &str) -> Option<(Tag<'_>, usize)> {
    let b = s.as_bytes();
    let mut i = 1;
    let closing = b.get(1) == Some(&b'/');
    if closing {
        i += 1;
    }
    if !b.get(i).is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let ns = i;
    while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b':') {
        i += 1;
    }
    let name = s[ns..i].to_ascii_lowercase();
    let as_ = i;
    let mut quote: Option<u8> = None;
    let limit = b.len().min(MAX_TAG_BYTES);
    while i < limit {
        match (quote, b[i]) {
            (None, b'"' | b'\'') => quote = Some(b[i]),
            (Some(q), c) if c == q => quote = None,
            (None, b'>') => break,
            (None, b'<') => return None,
            _ => {}
        }
        i += 1;
    }
    if i >= limit {
        return None;
    }
    let attrs = &s[as_..i];
    let self_closing = attrs.trim_end().ends_with('/');
    Some((
        Tag {
            name,
            closing,
            self_closing,
            attrs,
        },
        i + 1,
    ))
}

/// Find the end of a raw-text element (`script`, `style`): the byte just after `</name ... >`.
/// The search is case-insensitive and allocates nothing.
fn raw_text_end(s: &str, name: &str) -> usize {
    let b = s.as_bytes();
    let n = name.as_bytes();
    let mut from = 0;
    while let Some(off) = s[from..].find("</") {
        let i = from + off;
        let tail = &b[i + 2..];
        if tail.len() >= n.len() && tail[..n.len()].eq_ignore_ascii_case(n) {
            return s[i..].find('>').map_or(s.len(), |j| i + j + 1);
        }
        from = i + 2;
    }
    s.len()
}

fn heading_level(name: &str) -> Option<usize> {
    match name.as_bytes() {
        [b'h', d @ b'1'..=b'6'] => Some((d - b'0') as usize),
        _ => None,
    }
}

/// A parsed MediaWiki page as plain text. Paragraphs, lists, definition lists and headings keep their lines
/// (`- ` for list items, two spaces per `<dd>` level, `== Title ==` for headings, one `=` per level); a
/// table row is one line with tab-separated cells; `<pre>` keeps its whitespace; external links read
/// `text (href)`; scripts, styles, comments, reference markers, edit links and the TOC are dropped.
/// Entities are decoded once.
pub fn html_to_text(html: &str) -> String {
    let mut out = Out::default();
    // (tag name, depth) of an element being skipped with everything inside it. Depth counts only that tag
    // name, so an unclosed same-name element inside drops the rest of the page; acceptable because the API
    // flags already remove the TOC and edit links, and references always close.
    let mut skip: Option<(String, usize)> = None;
    let mut i = 0;
    while i < html.len() {
        let rest = &html[i..];
        let Some(lt) = rest.find('<') else {
            if skip.is_none() {
                out.text(rest);
            }
            break;
        };
        if lt > 0 {
            if skip.is_none() {
                out.text(&rest[..lt]);
            }
            i += lt;
            continue;
        }
        if rest.starts_with("<!--") {
            i += rest.find("-->").map_or(rest.len(), |j| j + 3);
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            i += rest.find('>').map_or(rest.len(), |j| j + 1);
            continue;
        }
        let Some((tag, len)) = parse_tag(rest) else {
            if skip.is_none() {
                out.text("<");
            }
            i += 1;
            continue;
        };
        i += len;
        let name = tag.name.as_str();
        let void = VOID.contains(&name) || tag.self_closing;
        if !tag.closing && (name == "script" || name == "style") {
            if !tag.self_closing {
                i += raw_text_end(&html[i..], name);
            }
            continue;
        }
        if let Some((sname, depth)) = skip.as_mut() {
            if sname == name {
                if tag.closing {
                    *depth -= 1;
                    if *depth == 0 {
                        skip = None;
                    }
                } else if !void {
                    *depth += 1;
                }
            }
            continue;
        }
        if !tag.closing
            && ((name == "sup" && has_class(tag.attrs, "reference"))
                || has_class(tag.attrs, "mw-editsection")
                || attr(tag.attrs, "id").is_some_and(|v| v == "toc"))
        {
            if !void {
                skip = Some((tag.name.clone(), 1));
            }
            continue;
        }
        handle_tag(&mut out, &tag);
    }
    out.finish()
}

fn handle_tag(out: &mut Out, tag: &Tag<'_>) {
    let name = tag.name.as_str();
    if name == "br" {
        out.newline();
        return;
    }
    if let Some(level) = heading_level(name) {
        let marks = "=".repeat(level.max(2));
        if tag.closing {
            out.trim_end_spaces();
            out.space = false;
            out.buf.push(' ');
            out.buf.push_str(&marks);
            out.blank_line();
        } else {
            out.blank_line();
            out.push_raw(&marks);
            out.buf.push(' ');
        }
        return;
    }
    match (name, tag.closing) {
        ("td" | "th", false) => {
            if out.cell == 0 {
                if out.cells_in_row > 0 {
                    out.trim_end_spaces();
                    out.space = false;
                    out.buf.push('\t');
                }
                out.cells_in_row += 1;
            } else {
                out.space = true;
            }
            out.cell += 1;
        }
        ("td" | "th", true) => {
            out.cell = out.cell.saturating_sub(1);
            out.space = out.cell > 0;
        }
        ("tr", _) => {
            out.newline();
            if out.cell == 0 {
                out.cells_in_row = 0;
            }
        }
        ("pre", false) => {
            out.newline();
            out.pre += 1;
        }
        ("pre", true) => {
            // Ends the line without trimming the block's own trailing spaces.
            out.newline();
            out.pre = out.pre.saturating_sub(1);
        }
        ("ul" | "ol", false) => {
            out.newline();
            out.list += 1;
        }
        ("ul" | "ol", true) => {
            out.list = out.list.saturating_sub(1);
            out.newline();
        }
        ("li", false) => {
            out.newline();
            if out.cell == 0 {
                let indent = "  ".repeat(out.list.clamp(1, MAX_INDENT_DEPTH) - 1);
                out.buf.push_str(&indent);
                out.buf.push_str("- ");
            }
        }
        ("dd", false) => {
            out.newline();
            out.dd += 1;
            if out.cell == 0 {
                out.buf.push_str(&"  ".repeat(out.dd.min(MAX_INDENT_DEPTH)));
            }
        }
        ("dd", true) => {
            out.dd = out.dd.saturating_sub(1);
            out.newline();
        }
        ("a", false) => {
            if out.links.len() >= MAX_LINK_DEPTH {
                out.untracked_links += 1;
                return;
            }
            let href = if has_class(tag.attrs, "external") {
                attr(tag.attrs, "href").map(|h| decode_entities(&h))
            } else {
                None
            };
            out.links.push((href, out.buf.len()));
        }
        ("a", true) => {
            if out.untracked_links > 0 {
                out.untracked_links -= 1;
                return;
            }
            if let Some((Some(href), start)) = out.links.pop() {
                let text = out.buf.get(start..).unwrap_or("").trim();
                let (empty, same) = (text.is_empty(), text == href);
                if empty {
                    out.push_raw(&href);
                } else if !same {
                    // Whitespace at the end of the link text stays pending for whatever follows.
                    let pending = out.space;
                    out.space = false;
                    out.trim_end_spaces();
                    out.buf.push_str(" (");
                    out.buf.push_str(&href);
                    out.buf.push(')');
                    out.space = pending;
                }
            }
        }
        (n, _) if BLOCK.contains(&n) => out.newline(),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_text(name: &str) -> String {
        let path = format!("{}/tests/fixtures/wiki/{name}", env!("CARGO_MANIFEST_DIR"));
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        v["parse"]["text"].as_str().unwrap().to_string()
    }

    fn report_snippets() -> Vec<(String, String)> {
        let path = format!(
            "{}/tests/fixtures/wiki/search_report.json",
            env!("CARGO_MANIFEST_DIR")
        );
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        v["query"]["search"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| {
                (
                    h["title"].as_str().unwrap().to_string(),
                    h["snippet"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    fn snippet(title: &str) -> String {
        report_snippets()
            .into_iter()
            .find(|(t, _)| t == title)
            .unwrap_or_else(|| panic!("{title} not in search_report.json"))
            .1
    }

    #[test]
    fn about_address_block_stays_multi_line() {
        let t = html_to_text(&fixture_text("parse_about.json"));
        assert!(
            t.contains(
                "  FlyBase-Harvard\n  Biological Laboratories, Harvard University\n  16 Divinity Avenue\n  Cambridge, Massachusetts 02138\n  USA"
            ),
            "address block not found"
        );
        // <pre> keeps its leading spaces and line breaks; non-ASCII names survive.
        assert!(t.contains("   Norbert Perrimon (PI)\n   Gil dos Santos\n"));
        assert!(t.contains("   Katja Röper (Co-PI)\n"));
        // External link: text (href).
        assert!(t.contains("contact FlyBase (http://flybase.org/contact/email) form"));
        assert!(t.contains("== FlyBase Consortium =="));
        for tag in ["<div", "<a ", "<dd", "<pre", "</"] {
            assert!(!t.contains(tag), "{tag} left in the text");
        }
    }

    #[test]
    fn report_guide_table_is_one_line_per_row() {
        let t = html_to_text(&fixture_text("parse_aberration_report.json"));
        assert!(t.contains("== General Information =="));
        let name_row = t
            .lines()
            .find(|l| l.starts_with("Name\t"))
            .expect("a Name row");
        assert_eq!(
            name_row,
            "Name\tThe valid full name that is used in FlyBase for the aberration."
        );
        // The Symbol cell holds three paragraphs; they stay on the row's one line.
        let symbol_row = t
            .lines()
            .find(|l| l.starts_with("Symbol\t"))
            .expect("a Symbol row");
        assert!(symbol_row.contains("for the aberration. The first part of the symbol"));
        assert!(symbol_row.ends_with("how aberration symbols are constructed."));
        assert_eq!(symbol_row.matches('\t').count(), 1);
    }

    #[test]
    fn pre_keeps_whitespace_and_cells_flatten_newlines() {
        assert_eq!(
            html_to_text("<p>a   b\nc</p><pre>  x\n    y  </pre><p>z</p>"),
            "a b c\n  x\n    y  \nz"
        );
        assert_eq!(
            html_to_text(
                "<table><tr><td>one\ntwo</td><td>three<br>four</td></tr><tr><th>H</th><td>v</td></tr></table>"
            ),
            "one two\tthree four\nH\tv"
        );
    }

    #[test]
    fn lists_headings_and_links() {
        assert_eq!(
            html_to_text("<ul><li>one</li><li>two<ul><li>deep</li></ul></li></ul>"),
            "- one\n- two\n  - deep"
        );
        assert_eq!(html_to_text("<ol><li>first</li></ol>"), "- first");
        assert_eq!(
            html_to_text(
                "<p>x</p><h2><span class=\"mw-headline\">Title</span></h2><h3>Sub</h3><p>y</p>"
            ),
            "x\n\n== Title ==\n\n=== Sub ===\n\ny"
        );
        assert_eq!(
            html_to_text(
                "see <a rel=\"nofollow\" class=\"external text\" href=\"http://x.org/?a=1&amp;b=2\">the site</a> or <a href=\"/wiki/Foo\" title=\"Foo\">Foo</a>"
            ),
            "see the site (http://x.org/?a=1&b=2) or Foo"
        );
        assert_eq!(
            html_to_text("<a class=\"external free\" href=\"http://x.org\">http://x.org</a>"),
            "http://x.org"
        );
    }

    #[test]
    fn entities_decode_once() {
        assert_eq!(
            html_to_text("a&#160;b &amp; c &#x41;&#65; &amp;lt; &lt;tag&gt; &quot;q&quot; &apos;"),
            "a b & c AA &lt; <tag> \"q\" '"
        );
        assert_eq!(decode_entities("&amp;lt;"), "&lt;");
        assert_eq!(
            decode_entities("&#xE9;&nbsp;&bogus; & &#; &#x;"),
            "é &bogus; & &#; &#x;"
        );
        assert_eq!(decode_entities("&#1114112;"), "&#1114112;");
    }

    #[test]
    fn dropped_elements() {
        let html = "<p>keep<!-- a <b>comment</b> --> one</p>\
            <script>if (a < b) { document.write('<div>no</div>'); }</script>\
            <STYLE>p { color: red }</STYLE>\
            <p>text<sup class=\"reference\" id=\"r1\"><a href=\"#c\"><sup>[1]</sup></a></sup> two</p>\
            <h2>Head<span class=\"mw-editsection\"><span>[</span><a>edit</a><span>]</span></span></h2>\
            <div id=\"toc\" class=\"toc\"><div><ul><li>Contents</li></ul></div></div>\
            <p>three</p>";
        assert_eq!(
            html_to_text(html),
            "keep one\ntext two\n\n== Head ==\n\nthree"
        );
    }

    #[test]
    fn uppercase_tags_and_gt_in_attributes() {
        assert_eq!(
            html_to_text(
                "<P>a</P><UL><LI title=\"x > y\">b</LI></UL><TABLE><TR><TD data-x='1>2'>c</TD><TD>d</TD></TR></TABLE>"
            ),
            "a\n- b\nc\td"
        );
        assert_eq!(html_to_text("1 < 2 and 3 > 2"), "1 < 2 and 3 > 2");
    }

    #[test]
    fn link_text_ending_in_space_and_control_references() {
        assert_eq!(
            html_to_text("<a class=\"external\" href=\"h\">x </a>y"),
            "x (h) y"
        );
        assert_eq!(
            decode_entities("a&#0;b&#7;c&#x9F;&#9;d"),
            "a&#0;b&#7;c&#x9F;\td"
        );
    }

    /// Output size and time stay linear on hostile nesting and unclosed markup (each case is about 1 MB,
    /// or 50,000 levels deep). Uncapped list indentation gave 400 MB for 160 KB of input. The time bound is generous for debug builds.
    #[test]
    fn hostile_html_is_linear() {
        let cases: Vec<(&str, String)> = vec![
            ("nested ul", "<ul><li>x".repeat(50_000)),
            ("nested dl", "<dl><dd>x".repeat(50_000)),
            ("unclosed tags", "<a x".repeat(250_000)),
            (
                "unterminated quote",
                format!("<a x=\"{}", "<a y".repeat(250_000)),
            ),
            ("quoted lt", "<a x=\"<\" ".repeat(100_000)),
            ("empty scripts", "<script></script>".repeat(60_000)),
            (
                "script near misses",
                format!("<SCRIPT>{}", "</scrip".repeat(150_000)),
            ),
            ("scripts and text", "<Script>x</sCRIPT>y".repeat(55_000)),
            (
                "nested external links",
                "<a class=\"external\" href=\"http://h\">t".repeat(25_000) + &"</a>".repeat(25_000),
            ),
        ];
        for (label, html) in cases {
            let started = std::time::Instant::now();
            let t = html_to_text(&html);
            let took = started.elapsed();
            assert!(
                took < std::time::Duration::from_secs(2),
                "{label}: {took:?}"
            );
            assert!(
                t.len() <= 3 * html.len(),
                "{label}: {} bytes out of {}",
                t.len(),
                html.len()
            );
        }
        let deep = html_to_text(&"<ul><li>x".repeat(50_000));
        let last = deep.lines().last().unwrap();
        assert_eq!(last, format!("{}- x", "  ".repeat(MAX_INDENT_DEPTH - 1)));
        assert_eq!(html_to_text("<a x"), "<a x");
        assert_eq!(
            html_to_text(&format!("<b{}>z", " ".repeat(MAX_TAG_BYTES))),
            "<b >z"
        );
    }

    #[test]
    fn broken_redirect_page_text() {
        let t = html_to_text(&fixture_text("parse_fbgn0000553.json"));
        assert_eq!(t, "- REDIRECT [[Dmel\\e(dp<up>v</up>)]]");
    }

    #[test]
    fn real_report_snippets() {
        assert_eq!(
            clean_snippet(&snippet("FlyBase:Report Help")),
            "Aberration Report Allele Report"
        );
        let combo = clean_snippet(&snippet("FlyBase:Combination Report"));
        assert!(
            combo.contains("Feature type | The type of feature described in the report."),
            "{combo}"
        );
        assert!(combo.starts_with("The Split System Combination report displays"));
        assert!(combo.contains("\"split system combination\""), "{combo}");
        let ins = clean_snippet(&snippet("FlyBase:Insertion Report"));
        assert!(
            ins.ends_with("information in the Insertion Report."),
            "{ins}"
        );
        assert_eq!(
            clean_snippet(&snippet("FlyBase:Pathway Report")),
            "FlyBase:Signaling Pathway Report"
        );
        for (title, s) in report_snippets() {
            let c = clean_snippet(&s);
            for bad in ["<", "[[", "]]", "'''", "&#039;", "\n", "#REDIRECT"] {
                assert!(!c.contains(bad), "{title}: {bad:?} in {c:?}");
            }
        }
    }

    #[test]
    fn snippet_markup() {
        assert_eq!(
            clean_snippet("{{Note|hidden}} after {{a|{{b}}}} x"),
            "after x"
        );
        assert_eq!(clean_snippet("cut {{Templ|arg"), "cut Templ|arg");
        assert_eq!(
            clean_snippet("#REDIRECT [[FlyBase:Stocks]]"),
            "FlyBase:Stocks"
        );
        assert_eq!(
            clean_snippet("== Heading ==\n* item\n# num\n: indent"),
            "Heading item num indent"
        );
        assert_eq!(
            clean_snippet("{|\n|-\n! A !! B\n| a || b\n|}"),
            "A | B a | b"
        );
        assert_eq!(
            clean_snippet(
                "[http://flybase.org/x <span class=\"searchmatch\">Stock</span> Collections]\n"
            ),
            "Stock Collections"
        );
        assert_eq!(
            clean_snippet("see [1] and [http://x.org]"),
            "see [1] and http://x.org"
        );
        assert_eq!(
            clean_snippet("''italic'' and '''bold'''"),
            "italic and bold"
        );
        assert_eq!(clean_snippet("[[FlyBase:Cut off|lab"), "lab");
        assert_eq!(clean_snippet(""), "");
    }

    #[test]
    fn error_codes_map_per_table() {
        assert_eq!(map_code("missingtitle"), WikiError::NotFound);
        assert_eq!(map_code("pagecannotexist"), WikiError::NotFound);
        for c in [
            "internal_api_error_DBQueryError",
            "readonly",
            "maxlag",
            "ratelimited",
        ] {
            assert!(matches!(map_code(c), WikiError::Unavailable(_)), "{c}");
        }
        assert_eq!(
            map_code("invalidtitle"),
            WikiError::Invalid("invalidtitle".into())
        );
        assert_eq!(map_code("badvalue"), WikiError::Invalid("badvalue".into()));
        assert_eq!(
            map_code("odd code <x>"),
            WikiError::Invalid("oddcodex".into()),
            "codes are reduced to identifier characters"
        );
    }

    #[test]
    fn errors_map_to_tool_classes() {
        assert_eq!(
            WikiError::NotFound.into_tool("get_wiki_page").code(),
            "not_found"
        );
        assert_eq!(
            WikiError::TooLarge.into_tool("get_wiki_page").code(),
            "limit_exceeded"
        );
        assert_eq!(
            WikiError::Unavailable("x".into())
                .into_tool("search_wiki")
                .code(),
            "backend_unavailable"
        );
        match WikiError::Invalid("badvalue".into()).into_tool("search_wiki") {
            ToolError::InvalidInput { arg, message, .. } => {
                assert_eq!(arg, "query");
                assert!(message.contains("badvalue"));
            }
            e => panic!("{e:?}"),
        }
        match WikiError::Invalid("invalidtitle".into()).into_tool("get_wiki_page") {
            ToolError::InvalidInput { arg, .. } => assert_eq!(arg, "title"),
            e => panic!("{e:?}"),
        }
    }

    #[test]
    fn https_base_is_rejected() {
        match WikiClient::new(
            "https://wiki.example/".parse().unwrap(),
            "wiki.flybase.org".into(),
        ) {
            Ok(_) => panic!("https base must be rejected"),
            Err(e) => assert!(e.to_string().contains("http://"), "{e}"),
        }
        assert!(WikiClient::new("http://127.0.0.1/".parse().unwrap(), "bad\nhost".into()).is_err());
    }

    #[test]
    fn api_url_keeps_base_path() {
        let c =
            WikiClient::new("http://127.0.0.1:9/mediawiki".parse().unwrap(), "h".into()).unwrap();
        assert_eq!(
            c.url(&[("action", "query")]).as_str(),
            "http://127.0.0.1:9/mediawiki/api.php?action=query&format=json&formatversion=2"
        );
        let c = WikiClient::new("http://127.0.0.1:9".parse().unwrap(), "h".into()).unwrap();
        assert!(
            c.url(&[])
                .as_str()
                .starts_with("http://127.0.0.1:9/api.php?")
        );
    }
}
