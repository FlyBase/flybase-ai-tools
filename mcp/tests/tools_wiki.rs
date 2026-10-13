//! search_wiki and get_wiki_page against a wiremock stand-in for the wiki's api.php, serving the captured
//! responses in tests/fixtures/wiki/. Every test that needs the wiki starts its own server with its own mock;
//! the default `base()` server leaves the wiki unconfigured.
mod support;
use flybase_mcp::backends::wiki::html_to_text;
use flybase_mcp::limits::MAX_JSON_BYTES;
use serde_json::{Value, json};
use support::server::{base, call, call_raw, client, client_at, start};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Value {
    let p = format!(
        "{}/tests/fixtures/wiki/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap()
}

/// A wiki-configured server in front of a fresh mock api.php.
async fn wiki_server() -> (MockServer, String) {
    let mock = MockServer::start().await;
    let mut cfg = support::test_config();
    cfg.wiki_base_url = Some(format!("{}/", mock.uri()).parse().unwrap());
    let base = start(cfg);
    (mock, base)
}

fn api() -> wiremock::MockBuilder {
    Mock::given(method("GET")).and(path("/api.php"))
}

async fn mount_siteinfo(mock: &MockServer, body: Value) {
    api()
        .and(query_param("meta", "siteinfo"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(mock)
        .await;
}

/// A search request for `srsearch` with the namespaces pinned; callers add matchers and `respond_with(ok(..))`.
fn search_req(srsearch: &str) -> wiremock::MockBuilder {
    api()
        .and(query_param("list", "search"))
        .and(query_param("srsearch", srsearch))
        .and(query_param("gsrsearch", srsearch))
        .and(query_param("srnamespace", "12|14|1000"))
        .and(query_param("gsrnamespace", "12|14|1000"))
}

fn ok(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

fn search_mock(srsearch: &str, body: Value) -> Mock {
    search_req(srsearch).respond_with(ok(body))
}

fn parse_mock(page: &str, body: Value) -> Mock {
    api()
        .and(query_param("action", "parse"))
        .and(query_param("page", page))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
}

/// A minimal `parse` response for a page the fixtures do not carry.
fn parse_body(title: &str, html: &str) -> Value {
    json!({"parse": {"title": title, "pageid": 1, "revid": 2, "text": html, "sections": [], "redirects": []}})
}

async fn error_of(base: &str, tool: &str, args: Value) -> Value {
    let c = client_at(base).await;
    let r = call_raw(&c, tool, args).await.unwrap();
    assert_eq!(r.is_error, Some(true), "{:?}", r.structured_content);
    r.structured_content.unwrap()
}

fn titles(v: &Value) -> Vec<&str> {
    v["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| h["title"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn search_resolves_redirects_and_cleans_snippets() {
    let (mock, base) = wiki_server().await;
    search_req("stock")
        .and(query_param("srlimit", "10"))
        .and(query_param("sroffset", "0"))
        .respond_with(ok(fixture("search_stock")))
        .expect(1)
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    let s = call(&c, "search_wiki", json!({"query": "stock"})).await;
    assert_eq!(
        titles(&s),
        [
            "Category:Stock Collections",
            "FlyBase:Stocks",
            "FlyBase:Stock Report"
        ]
    );
    let hits = s["hits"].as_array().unwrap();
    assert_eq!(hits[0]["snippet"], "Stock Collections");
    assert_eq!(
        hits[0]["url"],
        "https://wiki.flybase.org/wiki/Category:Stock_Collections"
    );
    assert_eq!(hits[0]["last_modified"], "2016-10-04T17:36:52Z");
    assert_eq!(
        hits[1]["url"],
        "https://wiki.flybase.org/wiki/FlyBase:Stocks"
    );
    assert_eq!(hits[1]["snippet"], "");
    assert_eq!(hits[1]["last_modified"], "2025-04-03T19:23:36Z");
    assert_eq!(
        hits[2]["snippet"],
        "..., when available, provides links to a collection's web site for additional stock information. ...her stock-related communications should be directed to the provider of the stock rather than to FlyBase."
    );
    assert_eq!(s["total"], 3);
    assert_eq!(s["has_more"], false);
    assert_eq!(s["matched_with_wildcards"], false);
    assert_eq!(s["truncated"], false);
    assert!(s["links"].as_object().unwrap().is_empty(), "{}", s["links"]);
    mock.verify().await;
}

/// A hit naming a redirect's target directly is not repeated, and a redirect to a missing page is dropped.
#[tokio::test]
async fn search_drops_duplicates_and_broken_redirects() {
    let (mock, base) = wiki_server().await;
    let mut body = fixture("search_stock");
    let q = &mut body["query"];
    q["search"].as_array_mut().unwrap().extend([
        json!({"ns": 1000, "title": "FlyBase:Stocks", "pageid": 100837,
               "snippet": "<span class=\"searchmatch\">Stocks</span>", "timestamp": "2025-04-03T19:23:36Z"}),
        json!({"ns": 1000, "title": "FlyBase:Old Stocks", "pageid": 5,
               "snippet": "", "timestamp": "2010-01-01T00:00:00Z"}),
    ]);
    q["redirects"]
        .as_array_mut()
        .unwrap()
        .push(json!({"from": "FlyBase:Old Stocks", "to": "FlyBase:Gone"}));
    q["pages"]
        .as_array_mut()
        .unwrap()
        .push(json!({"ns": 1000, "title": "FlyBase:Gone", "missing": true}));
    search_mock("stock", body).mount(&mock).await;
    let c = client_at(&base).await;
    let s = call(&c, "search_wiki", json!({"query": "stock"})).await;
    assert_eq!(
        titles(&s),
        [
            "Category:Stock Collections",
            "FlyBase:Stocks",
            "FlyBase:Stock Report"
        ]
    );
}

#[tokio::test]
async fn search_pages_with_offset() {
    let (mock, base) = wiki_server().await;
    search_req("report")
        .and(query_param("sroffset", "0"))
        .and(query_param("gsroffset", "0"))
        .respond_with(ok(fixture("search_report")))
        .mount(&mock)
        .await;
    search_req("report")
        .and(query_param("sroffset", "20"))
        .and(query_param("gsroffset", "20"))
        .and(query_param("srlimit", "25"))
        .respond_with(ok(fixture("search_report")))
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    let s = call(&c, "search_wiki", json!({"query": "report"})).await;
    assert_eq!(s["total"], 29);
    assert_eq!(s["offset"], 0);
    assert_eq!(s["has_more"], true);
    assert!(titles(&s).contains(&"FlyBase:Report Help"));
    assert!(
        titles(&s).contains(&"FlyBase:Signaling Pathway Report"),
        "redirect resolved: {:?}",
        titles(&s)
    );
    assert!(!titles(&s).contains(&"FlyBase:Pathway Report"));
    let help = s["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["title"] == "FlyBase:Report Help")
        .unwrap();
    assert_eq!(help["snippet"], "Aberration Report Allele Report");
    // offset 20 + 10 returned hits >= 29 total.
    let s = call(
        &c,
        "search_wiki",
        json!({"query": "report", "offset": 20, "limit": 25}),
    )
    .await;
    assert_eq!(s["offset"], 20);
    assert_eq!(s["has_more"], false);
}

#[tokio::test]
async fn search_argument_bounds() {
    let (mock, base) = wiki_server().await;
    api()
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;
    for (args, arg) in [
        (json!({"query": "a", "limit": 0}), "limit"),
        (json!({"query": "a", "limit": 26}), "limit"),
        (json!({"query": "a", "offset": 501}), "offset"),
        (json!({"query": "a".repeat(301)}), "query"),
        (json!({"query": "ab\u{7}c"}), "query"),
        (json!({"query": "a\nb"}), "query"),
        (json!({"query": "   "}), "query"),
    ] {
        let e = error_of(&base, "search_wiki", args.clone()).await;
        assert_eq!(e["error"], "invalid_input", "{args}");
        assert!(
            e["message"]
                .as_str()
                .unwrap()
                .starts_with(&format!("search_wiki: invalid {arg}")),
            "{e}"
        );
    }
    mock.verify().await;
}

#[tokio::test]
async fn zero_hits_retry_with_wildcards() {
    let (mock, base) = wiki_server().await;
    search_mock("FTP archive", fixture("search_ftp_archive_zero"))
        .expect(1)
        .mount(&mock)
        .await;
    search_mock("ftp* archive*", fixture("search_ftp_archive_wildcard"))
        .expect(1)
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    let s = call(&c, "search_wiki", json!({"query": "FTP archive"})).await;
    assert_eq!(titles(&s), ["FlyBase:Using FTP Archives"]);
    assert_eq!(s["matched_with_wildcards"], true);
    assert_eq!(s["total"], 1);
    assert_eq!(
        s["hits"][0]["url"],
        "https://wiki.flybase.org/wiki/FlyBase:Using_FTP_Archives"
    );
    mock.verify().await;
}

/// A query that already uses `*` or `"` is sent once; no hits is an empty result, not an error.
#[tokio::test]
async fn no_retry_for_operators_and_empty_is_not_an_error() {
    let (mock, base) = wiki_server().await;
    for q in ["ftp*", "\"FTP archive\""] {
        search_mock(q, fixture("search_ftp_archive_zero"))
            .expect(1)
            .mount(&mock)
            .await;
    }
    let c = client_at(&base).await;
    for q in ["ftp*", "\"FTP archive\""] {
        let s = call(&c, "search_wiki", json!({"query": q})).await;
        assert!(s["hits"].as_array().unwrap().is_empty());
        assert_eq!(s["total"], 0);
        assert_eq!(s["has_more"], false);
        assert_eq!(s["matched_with_wildcards"], false);
    }
    mock.verify().await;
}

#[tokio::test]
async fn page_round_trip_with_sections() {
    let (mock, base) = wiki_server().await;
    mount_siteinfo(&mock, fixture("siteinfo_rightsinfo")).await;
    parse_mock(
        "FlyBase:Balancer/Genotype variant Report",
        fixture("parse_balancer_genotype_variant_report"),
    )
    .mount(&mock)
    .await;
    parse_mock(
        "FlyBase:Aberration Report",
        fixture("parse_aberration_report"),
    )
    .mount(&mock)
    .await;
    let c = client_at(&base).await;
    let p = call(
        &c,
        "get_wiki_page",
        json!({"title": "FlyBase:Aberration Report"}),
    )
    .await;
    assert_eq!(p["title"], "FlyBase:Aberration Report");
    let url = "https://wiki.flybase.org/wiki/FlyBase:Aberration_Report";
    assert_eq!(p["url"], url);
    assert_eq!(p["links"]["wiki"], url);
    assert_eq!(p["revision"], 173638);
    assert_eq!(p["text_truncated"], false);
    assert_eq!(p["truncated"], false);
    let text = p["text"].as_str().unwrap();
    assert!(
        !text.contains("<td") && !text.contains("</") && !text.contains("&amp;"),
        "markup removed"
    );
    assert_eq!(p["total_chars"], text.chars().count());
    let raw = fixture("parse_aberration_report");
    assert_eq!(
        text,
        html_to_text(raw["parse"]["text"].as_str().unwrap()).trim_end()
    );
    assert_eq!(
        p["sections"][0],
        json!({"level": 2, "title": "General Information"})
    );
    assert_eq!(
        p["sections"][3],
        json!({"level": 3, "title": "Genes Deleted/Disrupted"})
    );
    assert!(p.get("licence").is_none(), "empty rightsinfo: no licence");

    let b = call(
        &c,
        "get_wiki_page",
        json!({"title": "FlyBase:Balancer/Genotype variant Report"}),
    )
    .await;
    assert_eq!(
        b["url"],
        "https://wiki.flybase.org/wiki/FlyBase:Balancer/Genotype_variant_Report"
    );
    assert!(
        b["sections"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["title"] == "Synonyms & Secondary IDs"),
        "{}",
        b["sections"]
    );
}

#[tokio::test]
async fn long_page_is_cut_at_a_line_break() {
    let (mock, base) = wiki_server().await;
    parse_mock("FlyBase:About", fixture("parse_about"))
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    let p = call(&c, "get_wiki_page", json!({"title": "FlyBase:About"})).await;
    assert_eq!(p["text_truncated"], true);
    assert_eq!(p["truncated"], false, "the envelope did not cut anything");
    let text = p["text"].as_str().unwrap();
    let n = text.chars().count();
    assert!(n <= 20_000 && n > 19_000, "{n}");
    let full = html_to_text(fixture("parse_about")["parse"]["text"].as_str().unwrap());
    assert_eq!(p["total_chars"], full.chars().count());
    assert!(full.chars().count() > 20_000);
    assert!(full.starts_with(text));
    assert!(
        full[text.len()..]
            .trim_start_matches([' ', '\t'])
            .starts_with('\n'),
        "cut at a line break"
    );
    let p = call(
        &c,
        "get_wiki_page",
        json!({"title": "FlyBase:About", "max_chars": 60000}),
    )
    .await;
    // The whole page (56,275 characters) fits under 60,000.
    assert_eq!(p["text_truncated"], false);
    assert_eq!(p["text"].as_str().unwrap(), full);
}

#[tokio::test]
async fn cut_respects_multibyte_characters_and_the_json_cap() {
    let (mock, base) = wiki_server().await;
    parse_mock(
        "FlyBase:Accents",
        parse_body("FlyBase:Accents", &format!("<p>{}</p>", "é".repeat(30_000))),
    )
    .mount(&mock)
    .await;
    parse_mock(
        "FlyBase:Big",
        parse_body(
            "FlyBase:Big",
            &format!("<p>{}</p>", "🦀🦀🦀🦀é".repeat(14_000)),
        ),
    )
    .mount(&mock)
    .await;
    let c = client_at(&base).await;
    let p = call(
        &c,
        "get_wiki_page",
        json!({"title": "FlyBase:Accents", "max_chars": 101}),
    )
    .await;
    assert_eq!(p["text"], "é".repeat(101));
    assert_eq!(p["text_truncated"], true);
    assert_eq!(p["total_chars"], 30_000);
    let p = call(
        &c,
        "get_wiki_page",
        json!({"title": "FlyBase:Big", "max_chars": 60000}),
    )
    .await;
    assert_eq!(p["text"].as_str().unwrap().chars().count(), 60_000);
    assert_eq!(p["text_truncated"], true);
    assert_eq!(p["truncated"], false);
    assert!(serde_json::to_vec(&p).unwrap().len() <= MAX_JSON_BYTES);
}

#[tokio::test]
async fn links_and_redirects_resolve() {
    let (mock, base) = wiki_server().await;
    parse_mock(
        "FlyBase:Aberration Report",
        fixture("parse_aberration_report"),
    )
    .expect(1)
    .mount(&mock)
    .await;
    // Project:About -> FlyBase Wiki:About -> FlyBase:About (R1: only Special: and Media: are refused early).
    let mut about = fixture("parse_about");
    about["parse"]["redirects"] = json!([{"from": "FlyBase Wiki:About", "to": "FlyBase:About"}]);
    parse_mock("Project:About", about).mount(&mock).await;
    parse_mock(
        "help:Contents",
        parse_body("Help:Contents", "<p>Help pages.</p>"),
    )
    .mount(&mock)
    .await;
    parse_mock(
        "Drosophila: overview",
        parse_body("Drosophila: overview", "<p>A main-namespace page.</p>"),
    )
    .mount(&mock)
    .await;
    let c = client_at(&base).await;
    let p = call(
        &c,
        "get_wiki_page",
        json!({"title": "https://wiki.flybase.org/wiki/FlyBase:Aberration_Report?action=history#x"}),
    )
    .await;
    assert_eq!(p["title"], "FlyBase:Aberration Report");
    let p = call(&c, "get_wiki_page", json!({"title": "Project:About"})).await;
    assert_eq!(p["title"], "FlyBase:About");
    assert_eq!(p["url"], "https://wiki.flybase.org/wiki/FlyBase:About");
    let p = call(&c, "get_wiki_page", json!({"title": "help:Contents"})).await;
    assert_eq!(p["title"], "Help:Contents");
    assert_eq!(p["text"], "Help pages.");
    let p = call(
        &c,
        "get_wiki_page",
        json!({"title": "Drosophila: overview"}),
    )
    .await;
    assert_eq!(p["text"], "A main-namespace page.");
    mock.verify().await;
}

/// Every namespace outside main, Help, Category and FlyBase is refused on the title the wiki resolves.
#[tokio::test]
async fn unreadable_namespaces_are_refused() {
    let (mock, base) = wiki_server().await;
    for (sent, resolved) in [
        ("user:Spammer", "User:Spammer"),
        ("User:Spammer", "User:Spammer"),
        ("User talk:Spammer", "User talk:Spammer"),
        ("MediaWiki:Sidebar", "MediaWiki:Sidebar"),
        ("Image:X.png", "File:X.png"),
        ("GeoJson:X", "GeoJson:X"),
    ] {
        parse_mock(sent, parse_body(resolved, "<p>spam</p>"))
            .mount(&mock)
            .await;
    }
    // An allowed-looking title whose redirect lands in User: (the post-redirect check).
    let mut redirected = parse_body("User:Spammer", "<p>spam</p>");
    redirected["parse"]["redirects"] = json!([{"from": "FlyBase:Innocent", "to": "User:Spammer"}]);
    parse_mock("FlyBase:Innocent", redirected)
        .mount(&mock)
        .await;
    for input in [
        "user:Spammer",
        " User:Spammer",
        "User_talk:Spammer",
        "MediaWiki:Sidebar",
        "Image:X.png",
        "GeoJson:X",
        "https://wiki.flybase.org/wiki/%55ser:Spammer",
        "FlyBase:Innocent",
    ] {
        let e = error_of(&base, "get_wiki_page", json!({"title": input})).await;
        assert_eq!(e["error"], "invalid_input", "{input}");
        let m = e["message"].as_str().unwrap();
        assert!(m.starts_with("get_wiki_page: invalid title"), "{m}");
        assert!(
            m.contains("main, Help, Category and FlyBase"),
            "{input}: {m}"
        );
    }
}

#[tokio::test]
async fn early_refusals_never_reach_the_wiki() {
    let (mock, base) = wiki_server().await;
    api()
        .and(query_param("action", "parse"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&mock)
        .await;
    for (args, arg) in [
        (json!({"title": "a".repeat(256)}), "title"),
        (json!({"title": "Special:RecentChanges"}), "title"),
        (json!({"title": "Media:X.png"}), "title"),
        (json!({"title": "A\tB"}), "title"),
        (json!({"title": ""}), "title"),
        (
            json!({"title": "FlyBase:About", "max_chars": 0}),
            "max_chars",
        ),
        (
            json!({"title": "FlyBase:About", "max_chars": 60001}),
            "max_chars",
        ),
    ] {
        let e = error_of(&base, "get_wiki_page", args.clone()).await;
        assert_eq!(e["error"], "invalid_input", "{args}");
        assert!(
            e["message"]
                .as_str()
                .unwrap()
                .starts_with(&format!("get_wiki_page: invalid {arg}")),
            "{e}"
        );
    }
    mock.verify().await;
}

#[tokio::test]
async fn missing_broken_and_invalid_pages() {
    let (mock, base) = wiki_server().await;
    parse_mock("FBgn0000553", fixture("parse_fbgn0000553"))
        .mount(&mock)
        .await;
    parse_mock(
        "FlyBase:No Such Page Exists Here",
        fixture("error_missingtitle"),
    )
    .mount(&mock)
    .await;
    parse_mock("Foo<bar>", fixture("error_invalidtitle"))
        .mount(&mock)
        .await;
    let e = error_of(&base, "get_wiki_page", json!({"title": "FBgn0000553"})).await;
    assert_eq!(e["error"], "not_found", "broken redirect: {e}");
    assert!(e["message"].as_str().unwrap().contains("FBgn0000553"));
    let e = error_of(
        &base,
        "get_wiki_page",
        json!({"title": "FlyBase:No Such Page Exists Here"}),
    )
    .await;
    assert_eq!(e["error"], "not_found");
    assert!(
        e["message"]
            .as_str()
            .unwrap()
            .contains("'FlyBase:No Such Page Exists Here'; try search_wiki"),
        "{e}"
    );
    let e = error_of(&base, "get_wiki_page", json!({"title": "Foo<bar>"})).await;
    assert_eq!(e["error"], "invalid_input");
    assert!(
        e["message"].as_str().unwrap().contains("invalidtitle"),
        "{e}"
    );
}

#[tokio::test]
async fn licence_comes_from_rightsinfo() {
    let (mock, base) = wiki_server().await;
    mount_siteinfo(
        &mock,
        json!({"batchcomplete": true, "query": {"rightsinfo": {
            "url": "https://creativecommons.org/licenses/by/4.0/", "text": "CC BY 4.0"}}}),
    )
    .await;
    parse_mock(
        "FlyBase:Aberration Report",
        fixture("parse_aberration_report"),
    )
    .mount(&mock)
    .await;
    let c = client_at(&base).await;
    let p = call(
        &c,
        "get_wiki_page",
        json!({"title": "FlyBase:Aberration Report"}),
    )
    .await;
    assert_eq!(
        p["licence"],
        json!({"text": "CC BY 4.0", "url": "https://creativecommons.org/licenses/by/4.0/"})
    );
}

#[tokio::test]
async fn over_cap_page_is_limit_exceeded() {
    let (mock, base) = wiki_server().await;
    api()
        .and(query_param("action", "parse"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("x".repeat(flybase_mcp::backends::wiki::MAX_WIKI_BYTES + 1)),
        )
        .mount(&mock)
        .await;
    let e = error_of(
        &base,
        "get_wiki_page",
        json!({"title": "FlyBase:Fly Lab List"}),
    )
    .await;
    assert_eq!(e["error"], "limit_exceeded");
    assert!(
        e["message"]
            .as_str()
            .unwrap()
            .contains("https://wiki.flybase.org/wiki/FlyBase:Fly_Lab_List"),
        "{e}"
    );
}

#[tokio::test]
async fn unconfigured_wiki_is_backend_unavailable() {
    let c = client().await;
    for (tool, args) in [
        ("search_wiki", json!({"query": "balancer"})),
        ("get_wiki_page", json!({"title": "FlyBase:About"})),
    ] {
        let r = call_raw(&c, tool, args).await.unwrap();
        let e = r.structured_content.unwrap();
        assert_eq!(e["error"], "backend_unavailable", "{tool}");
        assert!(
            e["message"]
                .as_str()
                .unwrap()
                .contains("not configured on this server"),
            "{e}"
        );
    }
}

#[tokio::test]
async fn unreachable_wiki_is_backend_unavailable() {
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut cfg = support::test_config();
    cfg.wiki_base_url = Some(format!("http://127.0.0.1:{port}/").parse().unwrap());
    let base = start(cfg);
    let e = error_of(&base, "search_wiki", json!({"query": "secretquery"})).await;
    assert_eq!(e["error"], "backend_unavailable");
    assert!(!e.to_string().contains("secretquery"));
    let h: Value = reqwest::get(format!("{base}/health"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["wiki"], "unreachable");
    assert_eq!(h["status"], "ok", "wiki reachability is a separate flag");
}

#[tokio::test]
async fn second_identical_call_is_cached() {
    let (mock, base) = wiki_server().await;
    parse_mock(
        "FlyBase:Aberration Report",
        fixture("parse_aberration_report"),
    )
    .expect(1)
    .mount(&mock)
    .await;
    search_mock("stock", fixture("search_stock"))
        .expect(1)
        .mount(&mock)
        .await;
    let c = client_at(&base).await;
    let args = json!({"title": "FlyBase:Aberration Report"});
    let a = call(&c, "get_wiki_page", args.clone()).await;
    let b = call(&c, "get_wiki_page", args).await;
    assert_eq!(a, b);
    let a = call(&c, "search_wiki", json!({"query": "stock"})).await;
    let b = call(&c, "search_wiki", json!({"query": "stock"})).await;
    assert_eq!(a, b);
    mock.verify().await;
}

#[tokio::test]
async fn health_reports_wiki_state() {
    let h: Value = reqwest::get(format!("{}/health", base()))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["wiki"], "unconfigured");
    let (mock, base) = wiki_server().await;
    let h: Value = reqwest::get(format!("{base}/health"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["wiki"], "unreachable", "no siteinfo mock yet: {h}");
    assert_eq!(h["status"], "ok");
    mount_siteinfo(&mock, fixture("siteinfo_rightsinfo")).await;
    let h: Value = reqwest::get(format!("{base}/health"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(h["wiki"], "ok");
    assert_eq!(h["status"], "ok");
}

/// A page with thousands of long headings lists at most 200, each at most 200 characters, and the envelope
/// never has to cut anything.
#[tokio::test]
async fn sections_are_capped() {
    let (mock, base) = wiki_server().await;
    let mut body = parse_body("FlyBase:Headings", "<p>Body.</p>");
    body["parse"]["sections"] = json!(
        (0..5000)
            .map(|i| json!({"level": "2", "line": format!("<b>{i}</b> {}", "h".repeat(300)), "anchor": format!("a{i}")}))
            .collect::<Vec<_>>()
    );
    parse_mock("FlyBase:Headings", body).mount(&mock).await;
    let c = client_at(&base).await;
    let p = call(&c, "get_wiki_page", json!({"title": "FlyBase:Headings"})).await;
    let sections = p["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 200);
    assert_eq!(p["sections_total"], 5000);
    assert!(
        sections
            .iter()
            .all(|s| s["title"].as_str().unwrap().chars().count() <= 200)
    );
    assert!(sections[0]["title"].as_str().unwrap().starts_with("0 hhh"));
    assert_eq!(p["truncated"], false);
    assert!(serde_json::to_vec(&p).unwrap().len() <= MAX_JSON_BYTES);
}

/// A hung siteinfo costs concurrent page calls one short lookup, not one each in turn, and a failed lookup
/// is not repeated within the retry interval.
#[tokio::test]
async fn hung_licence_lookup_is_paid_once() {
    let (mock, base) = wiki_server().await;
    api()
        .and(query_param("meta", "siteinfo"))
        .respond_with(
            ok(fixture("siteinfo_rightsinfo")).set_delay(std::time::Duration::from_secs(8)),
        )
        .mount(&mock)
        .await;
    for t in ["FlyBase:A", "FlyBase:B", "FlyBase:C"] {
        parse_mock(t, parse_body(t, "<p>x</p>")).mount(&mock).await;
    }
    let c = client_at(&base).await;
    let started = std::time::Instant::now();
    let (a, b) = tokio::join!(
        call(&c, "get_wiki_page", json!({"title": "FlyBase:A"})),
        call(&c, "get_wiki_page", json!({"title": "FlyBase:B"})),
    );
    let both = started.elapsed();
    assert!(a.get("licence").is_none() && b.get("licence").is_none());
    assert!(
        both < std::time::Duration::from_millis(3500),
        "two concurrent calls took {both:?}; one 2 s lookup expected"
    );
    let siteinfo = |reqs: &[wiremock::Request]| {
        reqs.iter()
            .filter(|r| r.url.query().unwrap_or("").contains("meta=siteinfo"))
            .count()
    };
    let before = siteinfo(&mock.received_requests().await.unwrap());
    assert!((1..=2).contains(&before), "{before}");
    let started = std::time::Instant::now();
    let p = call(&c, "get_wiki_page", json!({"title": "FlyBase:C"})).await;
    assert!(p.get("licence").is_none());
    assert!(
        started.elapsed() < std::time::Duration::from_millis(1000),
        "the failed lookup is remembered: {:?}",
        started.elapsed()
    );
    assert_eq!(siteinfo(&mock.received_requests().await.unwrap()), before);
}
