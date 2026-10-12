mod support;
use serde_json::json;
use support::server::{call, call_raw, client};

#[tokio::test]
async fn notch_physical_and_genetic() {
    let c = client().await;
    let s = call(
        &c,
        "get_interactions",
        json!({"id": "FBgn0004647", "limit": 5}),
    )
    .await;
    assert_eq!(s["gene"]["id"], "FBgn0004647");
    let ph = &s["physical"];
    assert_eq!(ph["items"].as_array().unwrap().len(), 5);
    assert_eq!(ph["has_more"], true);
    assert_eq!(ph["items"][0]["partner_symbol"], "Abl");
    assert_eq!(ph["items"][0]["type"], "physical association");
    assert!(
        ph["items"][0]["detection"]
            .as_str()
            .unwrap()
            .contains("coimmunoprecipitation")
    );
    assert!(
        ph["items"][0]["references"]
            .as_str()
            .unwrap()
            .starts_with("FBrf")
    );
    let ge = &s["genetic"];
    assert_eq!(ge["items"].as_array().unwrap().len(), 5);
    assert_eq!(ge["items"][0]["partner_symbol"], "A122");
    assert!(
        ge["items"][0]["references"]
            .as_str()
            .unwrap()
            .starts_with("FBrf")
    );
}

#[tokio::test]
async fn kind_filter_and_paging() {
    let c = client().await;
    let a = call(
        &c,
        "get_interactions",
        json!({"id": "FBgn0004647", "kind": "physical", "limit": 3}),
    )
    .await;
    assert!(a.get("genetic").is_none() || a["genetic"].is_null());
    let b = call(
        &c,
        "get_interactions",
        json!({"id": "FBgn0004647", "kind": "physical", "limit": 3, "offset": 3}),
    )
    .await;
    assert_ne!(
        a["physical"]["items"][0]["interaction"],
        b["physical"]["items"][0]["interaction"]
    );
}

#[tokio::test]
async fn allele_genetic_interactions_come_from_phenotype_comparison() {
    let c = client().await;
    let s = call(
        &c,
        "get_interactions",
        json!({"id": "FBal0012701", "kind": "genetic", "limit": 3}),
    )
    .await;
    assert_eq!(s["gene"]["id"], "FBgn0004647");
    let items = s["genetic"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 3);
    assert!(items[0]["genotype"].as_str().unwrap().contains("N[55e11]"));
    assert!(
        items[0]["outcome"]
            .as_str()
            .unwrap()
            .contains("enhanceable")
    );
    assert!(items[0]["reference"].as_str().unwrap().starts_with("FBrf"));
    assert_eq!(items[0]["side"], "genotype1");
    assert_eq!(s["genetic"]["has_more"], true);
}

#[tokio::test]
async fn allele_physical_resolves_through_gene() {
    let c = client().await;
    let s = call(
        &c,
        "get_interactions",
        json!({"id": "FBal0012701", "kind": "physical", "limit": 2}),
    )
    .await;
    assert_eq!(s["physical"]["items"].as_array().unwrap().len(), 2);
    assert!(s["note"].as_str().unwrap().contains("gene"));
}

#[tokio::test]
async fn rejects_other_ids() {
    let c = client().await;
    let r = call_raw(&c, "get_interactions", json!({"id": "FBrf0227604"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "invalid_input");
    let r = call_raw(&c, "get_interactions", json!({"id": "FBgn9999999"}))
        .await
        .unwrap();
    assert_eq!(r.structured_content.unwrap()["error"], "not_found");
}

fn physical_keys(s: &serde_json::Value) -> Vec<(String, String)> {
    s["physical"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            (
                x["partner_id"].as_str().unwrap().to_owned(),
                x["interaction"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[tokio::test]
async fn physical_partners_are_current_and_unique() {
    let c = client().await;
    let s = call(
        &c,
        "get_interactions",
        json!({"id": "FBgn0004647", "kind": "physical", "limit": 100}),
    )
    .await;
    let s2 = call(
        &c,
        "get_interactions",
        json!({"id": "FBgn0004647", "kind": "physical", "limit": 100, "offset": 100}),
    )
    .await;
    let mut keys = physical_keys(&s);
    keys.extend(physical_keys(&s2));
    assert!(!keys.is_empty());
    // wg's -XP is also associated with an obsolete FBgn; only current partner genes are reported.
    assert!(
        keys.iter().all(|(p, _)| p != "FBgn0004009"),
        "obsolete partner ID returned"
    );
    let distinct: std::collections::HashSet<_> = keys.iter().collect();
    assert_eq!(
        distinct.len(),
        keys.len(),
        "duplicate (partner, interaction) rows"
    );
}

#[tokio::test]
async fn physical_pages_do_not_overlap() {
    let c = client().await;
    let a = call(
        &c,
        "get_interactions",
        json!({"id": "FBgn0004647", "kind": "physical", "limit": 50}),
    )
    .await;
    let b = call(
        &c,
        "get_interactions",
        json!({"id": "FBgn0004647", "kind": "physical", "limit": 50, "offset": 50}),
    )
    .await;
    let ka: std::collections::HashSet<_> = physical_keys(&a).into_iter().collect();
    let kb = physical_keys(&b);
    assert!(!kb.is_empty());
    assert!(
        kb.iter().all(|k| !ka.contains(k)),
        "pages share a (partner, interaction) key"
    );
}

#[tokio::test]
async fn self_interaction_lists_gene_as_partner() {
    if !support::full_db() {
        return;
    }
    // Abl (FBgn0000017) has self-interactions (role "self", or bait and prey on Abl-XP alone).
    let c = client().await;
    let s = call(
        &c,
        "get_interactions",
        json!({"id": "FBgn0000017", "kind": "physical", "limit": 100}),
    )
    .await;
    let items = s["physical"]["items"].as_array().unwrap();
    assert!(
        items.iter().any(|x| x["partner_id"] == "FBgn0000017"),
        "no self-interaction row for Abl"
    );
}

#[tokio::test]
async fn allele_only_in_genotype2_has_genetic_comparisons() {
    if !support::full_db() {
        return;
    }
    // par-1[N1S.alphaTub67C.GFP] appears only as the compared-with genotype (genotype2).
    let c = client().await;
    let s = call(
        &c,
        "get_interactions",
        json!({"id": "FBal0117404", "kind": "genetic"}),
    )
    .await;
    let items = s["genetic"]["items"].as_array().unwrap();
    assert!(!items.is_empty());
    assert_eq!(items[0]["side"], "genotype2");
}
