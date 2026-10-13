//! The MediaWiki client against wiremock, serving the captured responses in `tests/fixtures/wiki/`.
//! No database is needed.
use flybase_mcp::backends::wiki::{MAX_WIKI_BYTES, WikiClient, WikiError};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use wiremock::matchers::{header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const HOST: &str = "wiki.flybase.org";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/wiki/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn json(name: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(fixture(name), "application/json; charset=utf-8")
}

fn client(mock: &MockServer) -> WikiClient {
    WikiClient::new(mock.uri().parse().unwrap(), HOST.into()).unwrap()
}

/// The decoded query pairs of the only request the mock received.
async fn only_request_pairs(mock: &MockServer) -> Vec<(String, String)> {
    let reqs = mock.received_requests().await.unwrap();
    assert_eq!(reqs.len(), 1);
    reqs[0]
        .url
        .query_pairs()
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect()
}

fn pairs(v: &[(&str, &str)]) -> Vec<(String, String)> {
    v.iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test]
async fn search_sends_host_and_exact_pairs_and_decodes_the_fixture() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api.php"))
        .and(header("host", HOST))
        .respond_with(json("search_stock.json"))
        .expect(1)
        .mount(&mock)
        .await;
    let r = client(&mock).search("stock", 10, 0).await.unwrap();
    assert_eq!(
        only_request_pairs(&mock).await,
        pairs(&[
            ("action", "query"),
            ("list", "search"),
            ("srsearch", "stock"),
            ("srnamespace", "12|14|1000"),
            ("srlimit", "10"),
            ("sroffset", "0"),
            ("srprop", "snippet|timestamp"),
            ("srinfo", "totalhits"),
            ("generator", "search"),
            ("gsrsearch", "stock"),
            ("gsrnamespace", "12|14|1000"),
            ("gsrlimit", "10"),
            ("gsroffset", "0"),
            ("redirects", "1"),
            ("prop", "info"),
            ("format", "json"),
            ("formatversion", "2"),
        ])
    );
    assert_eq!(r.searchinfo.totalhits, 3);
    assert_eq!(r.search.len(), 3);
    assert_eq!(r.search[1].title, "FlyBase:Stock Collections");
    assert_eq!(r.search[1].snippet, "");
    assert_eq!(r.redirects.len(), 1);
    assert_eq!(r.redirects[0].from, "FlyBase:Stock Collections");
    assert_eq!(r.redirects[0].to, "FlyBase:Stocks");
    assert_eq!(r.pages.len(), 3);
    assert!(r.pages.iter().all(|p| !p.missing && !p.invalid));
    let stocks = r
        .pages
        .iter()
        .find(|p| p.title == "FlyBase:Stocks")
        .unwrap();
    assert_eq!(stocks.ns, 1000);
    assert_eq!(stocks.touched.as_deref(), Some("2025-04-03T19:23:36Z"));
}

#[tokio::test]
async fn zero_hit_and_wildcard_fixtures_decode() {
    let mock = MockServer::start().await;
    Mock::given(query_param("srsearch", "FTP archive"))
        .respond_with(json("search_ftp_archive_zero.json"))
        .mount(&mock)
        .await;
    Mock::given(query_param("srsearch", "ftp* archive*"))
        .respond_with(json("search_ftp_archive_wildcard.json"))
        .mount(&mock)
        .await;
    let c = client(&mock);
    let zero = c.search("FTP archive", 10, 0).await.unwrap();
    assert_eq!(zero.searchinfo.totalhits, 0);
    assert!(zero.search.is_empty() && zero.redirects.is_empty() && zero.pages.is_empty());
    let wild = c.search("ftp* archive*", 10, 0).await.unwrap();
    assert_eq!(wild.search[0].title, "FlyBase:Using FTP Archives");
}

#[tokio::test]
async fn missing_query_member_is_an_empty_search() {
    let mock = MockServer::start().await;
    Mock::given(path("/api.php"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(r#"{"batchcomplete":true}"#, "application/json"),
        )
        .mount(&mock)
        .await;
    let r = client(&mock).search("x", 10, 0).await.unwrap();
    assert_eq!(r.searchinfo.totalhits, 0);
    assert!(r.search.is_empty());
}

#[tokio::test]
async fn hostile_query_is_one_srsearch_value() {
    let mock = MockServer::start().await;
    Mock::given(path("/api.php"))
        .respond_with(json("search_ftp_archive_zero.json"))
        .mount(&mock)
        .await;
    let q = "a&action=edit|b#c%d=e é";
    client(&mock).search(q, 5, 20).await.unwrap();
    let got = only_request_pairs(&mock).await;
    let sr: Vec<&str> = got
        .iter()
        .filter(|(k, _)| k == "srsearch")
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(sr, vec![q]);
    assert_eq!(sr[0].as_bytes(), q.as_bytes(), "byte-exact");
    let gsr: Vec<&str> = got
        .iter()
        .filter(|(k, _)| k == "gsrsearch")
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(gsr, vec![q]);
    let actions: Vec<&str> = got
        .iter()
        .filter(|(k, _)| k == "action")
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(actions, vec!["query"], "no injected action pair");
    assert!(got.contains(&("srlimit".into(), "5".into())));
    assert!(got.contains(&("gsroffset".into(), "20".into())));
    assert_eq!(got.len(), 17, "no extra pairs: {got:?}");
}

#[tokio::test]
async fn page_sends_exact_pairs_and_decodes_every_parse_fixture() {
    for (title, file) in [
        ("FlyBase:Aberration Report", "parse_aberration_report.json"),
        ("FlyBase:About", "parse_about.json"),
        ("FlyBase:Batch Download", "parse_batch_download.json"),
        (
            "FlyBase:Balancer/Genotype variant Report",
            "parse_balancer_genotype_variant_report.json",
        ),
        ("FBgn0000553", "parse_fbgn0000553.json"),
    ] {
        let mock = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api.php"))
            .and(header("host", HOST))
            .respond_with(json(file))
            .expect(1)
            .mount(&mock)
            .await;
        let p = client(&mock).page(title).await.unwrap();
        assert_eq!(
            only_request_pairs(&mock).await,
            pairs(&[
                ("action", "parse"),
                ("page", title),
                ("redirects", "1"),
                ("prop", "text|sections|revid"),
                ("disablelimitreport", "1"),
                ("disableeditsection", "1"),
                ("disabletoc", "1"),
                ("format", "json"),
                ("formatversion", "2"),
            ]),
            "{title}"
        );
        assert_eq!(p.title, title);
        assert!(p.redirects.is_empty(), "{title}: redirects decode as empty");
        assert!(p.revid > 0 && p.pageid > 0, "{title}");
        assert!(!p.text.is_empty(), "{title}");
        if title == "FBgn0000553" {
            assert!(p.sections.is_empty());
        } else {
            assert!(!p.sections.is_empty(), "{title}");
            assert!(
                p.sections.iter().all(|s| s.level.parse::<u8>().is_ok()),
                "{title}: level is a numeric string"
            );
        }
    }
}

#[tokio::test]
async fn aberration_sections_keep_string_levels() {
    let mock = MockServer::start().await;
    Mock::given(path("/api.php"))
        .respond_with(json("parse_aberration_report.json"))
        .mount(&mock)
        .await;
    let p = client(&mock)
        .page("FlyBase:Aberration Report")
        .await
        .unwrap();
    assert_eq!(p.sections[0].level, "2");
    assert_eq!(p.sections[0].line, "General Information");
    assert_eq!(p.sections[0].anchor, "General_Information");
}

#[tokio::test]
async fn rights_and_health_use_siteinfo() {
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api.php"))
        .and(header("host", HOST))
        .respond_with(json("siteinfo_rightsinfo.json"))
        .expect(2)
        .mount(&mock)
        .await;
    let c = client(&mock);
    let r = c.rights().await.unwrap();
    assert_eq!((r.url.as_str(), r.text.as_str()), ("", ""));
    assert_eq!(
        only_request_pairs(&mock).await,
        pairs(&[
            ("action", "query"),
            ("meta", "siteinfo"),
            ("siprop", "rightsinfo"),
            ("format", "json"),
            ("formatversion", "2"),
        ])
    );
    assert!(c.healthy().await);
}

#[tokio::test]
async fn healthy_is_false_on_error_or_outage() {
    let mock = MockServer::start().await;
    Mock::given(path("/api.php"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock)
        .await;
    assert!(!client(&mock).healthy().await);
    let refused = WikiClient::new(
        format!("http://127.0.0.1:{}/", free_port().await)
            .parse()
            .unwrap(),
        HOST.into(),
    )
    .unwrap();
    assert!(!refused.healthy().await);
}

#[tokio::test]
async fn error_codes_map_per_table() {
    let mock = MockServer::start().await;
    for (title, file) in [
        ("Missing", "error_missingtitle.json"),
        ("Foo<bar>", "error_invalidtitle.json"),
        ("Special:RecentChanges", "error_pagecannotexist.json"),
    ] {
        Mock::given(query_param("page", title))
            .respond_with(json(file))
            .mount(&mock)
            .await;
    }
    for code in [
        "internal_api_error_DBQueryError",
        "readonly",
        "maxlag",
        "ratelimited",
        "somethingnew",
    ] {
        Mock::given(query_param("page", code))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!(r#"{{"error":{{"code":"{code}","info":"x"}}}}"#),
                "application/json",
            ))
            .mount(&mock)
            .await;
    }
    let c = client(&mock);
    assert_eq!(c.page("Missing").await.unwrap_err(), WikiError::NotFound);
    assert_eq!(
        c.page("Special:RecentChanges").await.unwrap_err(),
        WikiError::NotFound
    );
    assert_eq!(
        c.page("Foo<bar>").await.unwrap_err(),
        WikiError::Invalid("invalidtitle".into())
    );
    for code in [
        "internal_api_error_DBQueryError",
        "readonly",
        "maxlag",
        "ratelimited",
    ] {
        match c.page(code).await.unwrap_err() {
            WikiError::Unavailable(m) => assert!(m.contains(code), "{m}"),
            e => panic!("{code}: {e:?}"),
        }
    }
    assert_eq!(
        c.page("somethingnew").await.unwrap_err(),
        WikiError::Invalid("somethingnew".into())
    );
    // The tool-level classes.
    assert_eq!(
        c.page("Missing")
            .await
            .unwrap_err()
            .into_tool("get_wiki_page")
            .code(),
        "not_found"
    );
    assert_eq!(
        c.page("Foo<bar>")
            .await
            .unwrap_err()
            .into_tool("get_wiki_page")
            .code(),
        "invalid_input"
    );
    assert_eq!(
        c.page("readonly")
            .await
            .unwrap_err()
            .into_tool("get_wiki_page")
            .code(),
        "backend_unavailable"
    );
}

#[tokio::test]
async fn redirect_non_200_and_non_json_are_unavailable() {
    let mock = MockServer::start().await;
    Mock::given(query_param("page", "redirect"))
        .respond_with(
            ResponseTemplate::new(301).insert_header("location", "http://elsewhere.example/"),
        )
        .mount(&mock)
        .await;
    Mock::given(query_param("page", "html"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("<html>login</html>", "text/html"))
        .mount(&mock)
        .await;
    Mock::given(query_param("page", "500"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock)
        .await;
    Mock::given(query_param("page", "shape"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(r#"{"parse":{"title":"secretquery"}}"#, "application/json"),
        )
        .mount(&mock)
        .await;
    let c = client(&mock);
    match c.page("redirect").await.unwrap_err() {
        WikiError::Unavailable(m) => assert!(m.contains("redirect") && m.contains("301"), "{m}"),
        e => panic!("{e:?}"),
    }
    assert_eq!(
        mock.received_requests().await.unwrap().len(),
        1,
        "redirect not followed"
    );
    assert!(matches!(
        c.page("html").await,
        Err(WikiError::Unavailable(_))
    ));
    assert!(matches!(c.page("500").await, Err(WikiError::Unavailable(m)) if m == "HTTP 500"));
    match c.page("shape").await.unwrap_err() {
        WikiError::Unavailable(m) => assert!(!m.contains("secretquery"), "{m}"),
        e => panic!("{e:?}"),
    }
}

#[tokio::test]
async fn body_over_the_cap_is_too_large() {
    let mock = MockServer::start().await;
    Mock::given(path("/api.php"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(vec![b' '; MAX_WIKI_BYTES + 1], "application/json"),
        )
        .mount(&mock)
        .await;
    let e = client(&mock)
        .page("FlyBase:Fly Lab List")
        .await
        .unwrap_err();
    assert_eq!(e, WikiError::TooLarge);
    assert_eq!(e.into_tool("get_wiki_page").code(), "limit_exceeded");
}

#[tokio::test]
async fn body_at_the_cap_is_read() {
    let mock = MockServer::start().await;
    let body = fixture("siteinfo_rightsinfo.json");
    let mut padded = body.into_bytes();
    padded.resize(MAX_WIKI_BYTES, b' ');
    Mock::given(path("/api.php"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(padded, "application/json"))
        .mount(&mock)
        .await;
    assert!(client(&mock).rights().await.is_ok());
}

/// A server that answers with a chunked body (no Content-Length) longer than the cap, so the running total
/// is what stops the read.
#[tokio::test]
async fn chunked_body_over_the_cap_is_too_large() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut req = [0u8; 4096];
        let _ = sock.read(&mut req).await;
        let head = "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ntransfer-encoding: chunked\r\n\r\n";
        if sock.write_all(head.as_bytes()).await.is_err() {
            return;
        }
        let chunk = vec![b' '; 64 * 1024];
        let framed = [
            format!("{:x}\r\n", chunk.len()).into_bytes(),
            chunk,
            b"\r\n".to_vec(),
        ]
        .concat();
        for _ in 0..(MAX_WIKI_BYTES / (64 * 1024) + 2) {
            if sock.write_all(&framed).await.is_err() {
                return;
            }
        }
        let _ = sock.write_all(b"0\r\n\r\n").await;
    });
    let c = WikiClient::new(
        format!("http://127.0.0.1:{port}/").parse().unwrap(),
        HOST.into(),
    )
    .unwrap();
    assert_eq!(c.page("big").await.unwrap_err(), WikiError::TooLarge);
}

async fn free_port() -> u16 {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    l.local_addr().unwrap().port()
}

#[tokio::test]
async fn transport_errors_never_carry_the_query() {
    let c = WikiClient::new(
        format!("http://127.0.0.1:{}/", free_port().await)
            .parse()
            .unwrap(),
        HOST.into(),
    )
    .unwrap();
    for e in [
        c.search("secretquery", 10, 0).await.unwrap_err(),
        c.page("secretquery").await.unwrap_err(),
    ] {
        let WikiError::Unavailable(m) = &e else {
            panic!("{e:?}")
        };
        assert!(!m.contains("secretquery"), "{m}");
        assert!(!format!("{e:?}").contains("secretquery"));
        let t = e.into_tool("search_wiki");
        assert_eq!(t.code(), "backend_unavailable");
        assert!(!t.to_string().contains("secretquery"));
    }
}

#[test]
fn https_base_is_rejected_at_new() {
    let e = WikiClient::new("https://wiki.flybase.org/".parse().unwrap(), HOST.into())
        .err()
        .expect("https must be rejected");
    assert!(e.to_string().contains("http://"), "{e}");
}
