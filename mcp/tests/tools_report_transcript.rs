mod support;
use serde_json::json;
use support::server::{call, client};

#[tokio::test]
async fn transcript_default_sections() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBtr0070507"})).await;
    assert_eq!(s["type"], "transcript");
    let sec = &s["sections"];
    assert_eq!(sec["identity"]["symbol"], "N-RA");
    assert_eq!(sec["identity"]["type"], "mRNA");
    assert!(sec["identity"]["length"].as_u64().unwrap() > 0);
    if support::full_db() {
        assert_eq!(sec["identity"]["length"], 10150); // FB2026_02
    }
    assert_eq!(sec["gene"]["id"], "FBgn0004647");
    assert_eq!(sec["location"]["chromosome"], "X");
    let exons = sec["exons"].as_array().unwrap();
    assert_eq!(exons.len(), 9);
    assert_eq!(exons[0]["start"], 3134870);
    assert_eq!(exons[0]["end"], 3135827);
    assert_eq!(sec["cds"]["segments"].as_array().unwrap().len(), 9);
    assert_eq!(sec["cds"]["segments"][0]["start"], 3135668);
    assert!(sec["cds"]["length"].as_u64().unwrap() > 0);
    assert!(s["links"]["jbrowse"].as_str().unwrap().contains("loc=X:"));
}

/// A non-coding transcript has no CDS segments and a null coding length (FBtr0344909 is not in the fixture).
#[tokio::test]
async fn noncoding_transcript_cds_length_is_null() {
    if !support::full_db() {
        return;
    }
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBtr0344909", "sections": ["cds"]}),
    )
    .await;
    let cds = &s["sections"]["cds"];
    assert!(cds["segments"].as_array().unwrap().is_empty(), "{cds}");
    assert!(cds["length"].is_null(), "{cds}");
}

#[tokio::test]
async fn transcript_polypeptide_section() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBtr0070507", "sections": ["polypeptide", "utrs", "introns"]}),
    )
    .await;
    assert_eq!(s["sections"]["polypeptide"]["id"], "FBpp0070483");
    assert!(s["sections"]["polypeptide"]["length"].as_u64().unwrap() > 0);
    if support::full_db() {
        assert_eq!(s["sections"]["polypeptide"]["length"], 2703); // FB2026_02
    }
    assert_eq!(
        s["sections"]["utrs"]["five_prime"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(s["sections"]["introns"].as_array().unwrap().len(), 8);
}

#[tokio::test]
async fn polypeptide_default_sections() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBpp0070483", "limit_per_section": 5}),
    )
    .await;
    assert_eq!(s["type"], "polypeptide");
    let sec = &s["sections"];
    assert_eq!(sec["identity"]["symbol"], "N-PA");
    assert_eq!(sec["identity"]["molecular_weight"], "288851.1");
    assert_eq!(sec["gene"]["id"], "FBgn0004647");
    assert_eq!(sec["transcript"]["id"], "FBtr0070507");
    assert!(sec["length"].as_u64().unwrap() > 0);
    if support::full_db() {
        assert_eq!(sec["length"], 2703); // FB2026_02
    }
    let d = &sec["domains"];
    assert_eq!(d["items"].as_array().unwrap().len(), 5);
    assert_eq!(d["has_more"], true);
    assert_eq!(d["items"][0]["name"], "EGF-like domain");
    assert!(d["items"][0]["start"].as_u64().unwrap() >= 1);
    if support::full_db() {
        assert_eq!(d["items"][0]["start"], 61); // FB2026_02
    }
    assert!(
        d["items"][0]["accessions"]
            .as_str()
            .unwrap()
            .contains("INTERPRO:IPR000742")
    );
    // Per-exon domain parts (FB2026_03+) carry no accessions and must not appear.
    assert!(
        d["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|i| i["accessions"].is_string()),
        "{d}"
    );
}

#[tokio::test]
async fn polypeptide_external_links() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBpp0070483", "sections": ["external_links"]}),
    )
    .await;
    let links = s["sections"]["external_links"].as_array().unwrap();
    assert!(
        links
            .iter()
            .any(|l| l["database"] == "UniProt/Swiss-Prot" && l["accession"] == "P07207")
    );
}
