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
    assert_eq!(sec["identity"]["length"], 10150);
    assert_eq!(sec["gene"]["id"], "FBgn0004647");
    assert_eq!(sec["location"]["chromosome"], "X");
    let exons = sec["exons"].as_array().unwrap();
    assert_eq!(exons.len(), 9);
    assert_eq!(exons[0]["start"], 3134870);
    assert_eq!(exons[0]["end"], 3135827);
    assert_eq!(sec["cds"]["segments"].as_array().unwrap().len(), 9);
    assert_eq!(sec["cds"]["segments"][0]["start"], 3135668);
    assert!(s["links"]["jbrowse"].as_str().unwrap().contains("loc=X:"));
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
    assert_eq!(s["sections"]["polypeptide"]["length"], 2703);
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
    assert_eq!(sec["length"], 2703);
    let d = &sec["domains"];
    assert_eq!(d["items"].as_array().unwrap().len(), 5);
    assert_eq!(d["has_more"], true);
    assert_eq!(d["items"][0]["name"], "EGF-like domain");
    assert_eq!(d["items"][0]["start"], 61);
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
