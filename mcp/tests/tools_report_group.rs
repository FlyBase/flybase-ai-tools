mod support;
use serde_json::json;
use support::server::{call, client};

#[tokio::test]
async fn gene_group_default_sections() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBgg0000040"})).await;
    assert_eq!(s["type"], "gene_group");
    let sec = &s["sections"];
    assert_eq!(sec["identity"]["symbol"], "E3-RBR");
    assert_eq!(
        sec["identity"]["name"],
        "RING-BETWEEN-RING UBIQUITIN LIGASES"
    );
    assert_eq!(sec["identity"]["is_pathway"], false);
    assert!(
        sec["identity"]["description"]
            .as_str()
            .unwrap()
            .starts_with("E3 (ubiquitin) ligases")
    );
    if support::full_db() {
        // Member genes of E3-RBR are not fixture seeds; the fixture carries the group rows but no member features.
        assert_eq!(sec["members"]["count_shown"], 6);
        assert!(
            sec["members"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|m| m["id"] == "FBgn0017418")
        );
    } else {
        assert_eq!(sec["members"]["has_more"], false);
    }
    assert!(
        sec["hierarchy"]["parents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"] == "FBgg0000069" && p["relationship"] == "parent_grp")
    );
}

#[tokio::test]
async fn pathway_members_carry_labels() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBgg0001068", "sections": ["identity", "members"], "limit_per_section": 100}),
    )
    .await;
    assert_eq!(s["sections"]["identity"]["is_pathway"], true);
    let members = s["sections"]["members"]["items"].as_array().unwrap();
    let n = members
        .iter()
        .find(|m| m["id"] == "FBgn0004647")
        .expect("Notch is in the Notch pathway");
    assert!(
        n["label"]
            .as_str()
            .unwrap()
            .contains("NOTCH SIGNALING PATHWAY CORE COMPONENTS")
    );
}

#[tokio::test]
async fn gene_group_optional_sections() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBgg0000040", "sections": ["cvterms", "references", "synonyms"]}),
    )
    .await;
    assert!(
        s["sections"]["cvterms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["term_id"] == "GO:0061630")
    );
    // 5 grp_pub links, 2 of them to obsolete pubs (FBrf0225003, FBrf0224297), which references excludes.
    assert_eq!(s["sections"]["references"]["count_shown"], 3);
    assert!(
        s["sections"]["synonyms"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == "RBR")
    );
}

#[tokio::test]
async fn disease_model_default_sections() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBhh0000679"})).await;
    assert_eq!(s["type"], "disease_model");
    let sec = &s["sections"];
    assert_eq!(
        sec["identity"]["name"],
        "cancer, epithelial, NOTCH-SCRIB-related"
    );
    assert_eq!(sec["identity"]["category"], "specific entity");
    if support::full_db() {
        assert_eq!(sec["human_genes"][0]["id"], "FBgn0263009"); // Hsap\SCRIB is not a fixture seed
    }
    assert!(
        sec["fly_models"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g["id"] == "FBgn0004647" && g["role"] == "dmel_gene_implicated")
    );
    assert_eq!(sec["do_terms"][0]["term_id"], "DOID:305"); // carcinoma
}

#[tokio::test]
async fn disease_model_optional_sections() {
    let c = client().await;
    let s = call(&c, "get_report", json!({"id": "FBhh0000001", "sections": ["do_terms", "related", "references", "descriptions"], "limit_per_section": 5})).await;
    assert_eq!(s["sections"]["do_terms"][0]["term_id"], "DOID:1289");
    assert!(
        s["sections"]["related"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["id"] == "FBhh0000003")
    );
    assert_eq!(s["sections"]["references"]["has_more"], true);
    assert!(
        s["sections"]["descriptions"]["cellular"]
            .as_str()
            .unwrap()
            .contains("polyglutamine")
    );
}

#[tokio::test]
async fn disease_variants() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBhh0000003", "sections": ["variants"], "limit_per_section": 2}),
    )
    .await;
    let v = &s["sections"]["variants"]["items"];
    assert_eq!(v[0]["allele_id"], "FBal0093284");
    assert_eq!(v[0]["designation"], "HTT:p.Gln18[75]");
}

#[tokio::test]
async fn disease_fly_models_are_genes() {
    let c = client().await;
    let s = call(
        &c,
        "get_report",
        json!({"id": "FBhh0000003", "sections": ["fly_models"]}),
    )
    .await;
    // Shape holds everywhere; the fixture carries no humanhealth_feature rows for FBhh0000003, so it is empty there.
    let models = s["sections"]["fly_models"]
        .as_array()
        .expect("fly_models is an array");
    assert!(
        models
            .iter()
            .all(|g| g["id"].as_str().unwrap().starts_with("FBgn")),
        "{models:?}"
    );
    if support::full_db() {
        // FBhh0000003 also links 18 'disease implicated variant' features (HGVS names such as HTT:p.Gln18[103]); none may appear.
        assert_eq!(models.len(), 1, "{models:?}");
    }
}
