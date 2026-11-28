use super::*;
use crate::{
    backends::{cache::Caches, chado::Chado},
    config::Config,
    limits::Limits,
};
use rmcp::handler::server::wrapper::Parameters;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::{fmt::Write, fs, path::PathBuf};

const RELEASE: &str = "FB2026_03";

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "flybase-crossrefs-{}-{}",
            std::process::id(),
            rand::random::<u64>(),
        ));
        fs::create_dir(&path).unwrap();
        let fixture = Self(path);
        let root = &fixture.0;
        fs::create_dir(root.join("relationship-sources")).unwrap();
        fs::write(
            root.join("report-membership.tsv"),
            format!("# release: {RELEASE}\nFBlc0000001\nFBst0000002\nFBst0000004\nFBtc0000012\n# complete\n"),
        )
        .unwrap();

        let maps = Self::index(
            root.join("maps.sqlite"),
            json!({
                "symbols": {"rows": 6},
                "symbol_ids": {"rows": 4},
            }),
        );
        maps.execute_batch(
            "create table maps(source text,id text,value text,primary key(source,id));\
             insert into maps values\
               ('symbols','FBgn0000055','Adh'),\
               ('symbols','FBgn0004000','dense source'),\
               ('symbols','FBgn0004001','no relationships'),\
               ('symbols','FBgn0004002','another dense source'),\
               ('symbols','FBgn0004003','FBgn0004003'),\
               ('symbols','utf8 FBgn0004003','different display'),\
               ('symbol_ids','Adh','FBgn0000055'),\
               ('symbol_ids','alpha[example]','FBgn0000055'),\
               ('symbol_ids','Fbxl4','FBgn0000055'),\
               ('symbol_ids','Omega[[example]]','FBgn0000055');",
        )
        .unwrap();

        let mut general = String::from(
            "FBgn0000055\tFBtr0000003\tAdh-RC\n\t\tFBmc0000002\t\n\t\tFBms0000001\n\t\tFBtr0000003\n",
        );
        let large_offset = general.len();
        for i in 1..=LINKS_PER_RECORD + 2 {
            let source = if i == 1 { "FBgn0004000" } else { "" };
            writeln!(general, "{source}\tFBal{i:07}").unwrap();
        }
        let dense_offset = general.len();
        for i in 1..=3_600 {
            let source = if i == 1 { "FBgn0004002" } else { "" };
            writeln!(general, "{source}\tFBal{i:07}").unwrap();
        }
        let mut sources = serde_json::Map::new();
        for (name, contents, records) in [
            ("general", general.as_str(), 3),
            (
                "references",
                "FBrf0015431\tFBgn0000055\n\t\tFBst0000002\n",
                1,
            ),
            (
                "stock_objects",
                "FBst0000002\tFBal0000021\nFBst0000002\nFBst0000002\tFBal0000078_bad_target\nFBst0000002\tFBal0005583\nFBst0000002\tFBba0000025\nFBst0000002\tFBsv:0000101\nFBst0000002\tFBsn0000277\n",
                1,
            ),
        ] {
            let name_path = format!("{name}.cat");
            fs::write(root.join("relationship-sources").join(&name_path), contents).unwrap();
            sources.insert(
                name.into(),
                json!({
                    "file": name_path, "records": records, "bytes": contents.len(),
                    "sha256": format!("{:x}", Sha256::digest(contents.as_bytes())),
                }),
            );
        }
        let offsets = Self::index(
            root.join("relationship-index.sqlite"),
            Value::Object(sources),
        );
        offsets
            .execute_batch(
                "create table offsets(source text,id text,position integer,primary key(source,id));\
             insert into offsets values ('general','FBgn0000055',0),\
               ('references','FBrf0015431',0),('stock_objects','FBst0000002',0);",
            )
            .unwrap();
        offsets
            .execute(
                "insert into offsets values ('general','FBgn0004000',?1)",
                [i64::try_from(large_offset).unwrap()],
            )
            .unwrap();
        offsets
            .execute(
                "insert into offsets values ('general','FBgn0004002',?1)",
                [i64::try_from(dense_offset).unwrap()],
            )
            .unwrap();
        fixture
    }

    fn index(path: PathBuf, sources: Value) -> Connection {
        let db = Connection::open(path).unwrap();
        db.execute_batch(
            "create table metadata(key text primary key,value text);\
             insert into metadata values ('schema','1'),('complete','true'),('scope','full-release');",
        ).unwrap();
        db.execute("insert into metadata values ('release',?1)", [RELEASE])
            .unwrap();
        db.execute(
            "insert into metadata values ('sources',?1)",
            [sources.to_string()],
        )
        .unwrap();
        db
    }

    fn load(&self) -> RecordXrefs {
        RecordXrefs::load(&self.0, RELEASE).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn page(
    catalogs: &RecordXrefs,
    ids: &[&str],
    offset: usize,
    start_link: usize,
    targets: &[&str],
    tsv: bool,
) -> Value {
    record_page(
        catalogs,
        RELEASE,
        ids[offset..].iter().map(|id| id.to_string()).collect(),
        &targets.iter().map(|s| s.to_string()).collect(),
        PageStart {
            offset,
            start_link,
            total: ids.len(),
        },
        tsv,
    )
    .unwrap()
    .structured_content
    .unwrap()
}

#[test]
fn catalog_preserves_source_rules_and_input_order() {
    let fixture = Fixture::new();
    let catalogs = fixture.load();
    let ids = [
        " FBgn0000055 ",
        "&agr;<up>example</up>",
        "FBst0000002",
        "FBrf0015431",
        "FBst0000004",
        "FBrf9999999",
        "FBst0000001",
        "Adh",
        "FBgn0004001",
        "adh",
        " fbGN0000055 ",
        "Fbxl4",
        "&OHgr;<down>example</down>",
        "FBlc0000001",
        "FBtc0000012",
        "FBtc9999999",
        "FBgn0004003",
    ];
    let result = page(&catalogs, &ids, 0, 0, &[], false);
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), ids.len());
    assert_eq!(rows[0]["input"], " FBgn0000055 ");
    assert_eq!(rows[0]["id"], "FBgn0000055");
    assert_eq!(rows[1]["input"], "&agr;<up>example</up>");
    assert_eq!(rows[1]["id"], rows[0]["id"]);
    assert_eq!(rows[7]["input"], "Adh");
    assert_eq!(rows[7]["cross_references"], rows[0]["cross_references"]);
    assert_eq!(
        rows[0]["cross_references"],
        json!({
            "FBtp": ["FBmc0000002", "FBms0000001"],
            "FBtr": ["FBtr0000003"]
        })
    );
    assert_eq!(
        rows[2]["cross_references"],
        json!({
            "FBal": ["FBal0000021", "FBal0005583"],
            "FBba": ["FBba0000025"]
        })
    );
    assert_eq!(
        rows[3]["cross_references"],
        json!({"FBgn": ["FBgn0000055"]})
    );
    for index in [11, 12] {
        assert_eq!(rows[index]["input"], ids[index]);
        assert_eq!(rows[index]["cross_references"], rows[0]["cross_references"]);
    }
    for index in [4, 5, 8, 13, 14, 16] {
        assert_eq!(rows[index]["status"], "resolved");
        assert_eq!(rows[index]["cross_references"], json!({}));
    }
    for index in [6, 9, 10, 15] {
        assert_eq!(rows[index]["status"], "unknown");
        assert!(rows[index]["id"].is_null());
    }
    assert_eq!(
        (
            result["next_offset"].clone(),
            result["next_link_offset"].clone()
        ),
        (Value::Null, Value::Null)
    );
    assert_eq!(
        page(
            &catalogs,
            &["FBst0000002", " \u{2002}=1+1"],
            0,
            0,
            &["FBba"],
            true
        )["tsv"],
        "input\tid\tstatus\tlink_offset\tFBba\nFBst0000002\tFBst0000002\tresolved\t0\tFBba0000025\n' \u{2002}=1+1\t\tunknown\t0\t\n",
    );
}

#[test]
fn continuations_finish_the_record_then_advance_to_next_input() {
    let fixture = Fixture::new();
    let catalogs = fixture.load();
    let ids = ["FBgn0000055", "FBgn0004000", "FBgn0004000"];
    let first = page(&catalogs, &ids, 0, 0, &["FBal"], false);
    let links = first["rows"][1]["cross_references"]["FBal"]
        .as_array()
        .unwrap();
    assert_eq!(links.len(), LINKS_PER_RECORD);
    assert_eq!(links.first().unwrap(), "FBal0000001");
    assert_eq!(links.last().unwrap(), "FBal0004000");
    assert_eq!(first["next_offset"], 1);
    assert_eq!(first["next_link_offset"], LINKS_PER_RECORD);
    let second = page(&catalogs, &ids, 1, LINKS_PER_RECORD, &["FBal"], false);
    assert_eq!(second["rows"][0]["link_offset"], LINKS_PER_RECORD);
    assert_eq!(
        second["rows"][0]["cross_references"]["FBal"],
        json!(["FBal0004001", "FBal0004002"])
    );
    assert_eq!(second["rows"][1]["input"], ids[2]);
    assert_eq!(second["rows"][1]["link_offset"], 0);
    assert_eq!(second["next_offset"], 2);
    assert_eq!(second["next_link_offset"], LINKS_PER_RECORD);
    let last = page(&catalogs, &ids, 2, LINKS_PER_RECORD, &["FBal"], false);
    assert!(last["next_offset"].is_null());
    assert!(last["next_link_offset"].is_null());
    let mut bytes_left = MAX_SCAN_BYTES;
    let invalid = load_row(
        &catalogs,
        ids[2].into(),
        &BTreeSet::new(),
        usize::MAX,
        &mut bytes_left,
    );
    assert!(matches!(
        invalid,
        Err(ToolError::InvalidInput {
            arg: "link_offset",
            ..
        })
    ));
    assert!(
        catalogs
            .associations(ids[2], &BTreeSet::new(), &mut 8)
            .unwrap()
            .is_none()
    );
}

#[test]
fn catalog_requires_matching_release_and_verified_sources() {
    let fixture = Fixture::new();
    assert!(RecordXrefs::load(&fixture.0, "FB2026_02").is_err());
    let general = fixture.0.join("relationship-sources/general.cat");
    let mut data = fs::read(&general).unwrap();
    data[0] = b'Z';
    fs::write(&general, data).unwrap();
    let result = RecordXrefs::load(&fixture.0, RELEASE);
    assert!(
        matches!(result, Err(err) if err.to_string().contains("general catalog hash mismatch"))
    );

    let index = Connection::open(fixture.0.join("relationship-index.sqlite")).unwrap();
    index
        .execute("delete from metadata where key = 'sources'", [])
        .unwrap();
    let result = RecordXrefs::load(&fixture.0, RELEASE);
    assert!(matches!(result, Err(err) if {
        let message = err.to_string();
        message.contains("metadata key sources") && message.contains("relationship-index.sqlite")
    }));
}

#[test]
fn catalog_rejects_incomplete_membership_export() {
    let fixture = Fixture::new();
    let file = fixture.0.join("report-membership.tsv");
    let original = fs::read_to_string(&file).unwrap();
    for incomplete in [
        original.trim_end_matches("# complete\n").to_owned(),
        format!("{original}FBtc0000013\n"),
    ] {
        fs::write(&file, incomplete).unwrap();
        assert!(RecordXrefs::load(&fixture.0, RELEASE).is_err());
    }
}

async fn call_batch(server: &FlyBaseServer, args: Value) -> Value {
    let params = serde_json::from_value(args).unwrap();
    server
        .batch_download(Parameters(params))
        .await
        .unwrap()
        .structured_content
        .unwrap()
}

#[tokio::test]
async fn batch_dispatches_both_modes_and_validates_mode_specific_inputs() {
    let cfg = Config::from_lookup(|key| {
        Some(
            match key {
                "FLYBASE_MCP_BIND" => "127.0.0.1:0",
                "FLYBASE_MCP_PUBLIC_URL" => "http://127.0.0.1",
                "FLYBASE_RELEASE" => RELEASE,
                "CHADO_URL" => "postgres://127.0.0.1/flybase",
                _ => return None,
            }
            .to_owned(),
        )
    })
    .unwrap();
    let chado = Chado::connect(&cfg).await.unwrap();
    let mut server = FlyBaseServer::new(
        chado,
        Limits::new(),
        RELEASE.into(),
        Arc::new(Caches::new(0)),
        None,
        None,
    );
    let default_options = call_batch(&server, json!({"list_options": true})).await;
    assert!(default_options["fields"]["gene"].is_array());
    let xrefs_options = json!({"mode": "record_cross_references", "list_options": true});
    assert_eq!(
        call_batch(&server, xrefs_options.clone()).await["available"],
        false
    );
    let xrefs = json!({"mode": "record_cross_references", "ids": ["Fbxl4"]});
    assert_eq!(
        call_batch(&server, xrefs.clone()).await["error"],
        "backend_unavailable"
    );
    let fixture = Fixture::new();
    server.record_xrefs = Some(Arc::new(fixture.load()));
    assert_eq!(call_batch(&server, xrefs_options).await["available"], true);
    let valid = call_batch(&server, xrefs).await;
    assert_eq!(valid["rows"][0]["input"], "Fbxl4");
    assert_eq!(valid["rows"][0]["id"], "FBgn0000055");
    for args in [
        json!({"ids": ["FBgn0000055"], "fields": ["name"], "link_offset": 1}),
        json!({"mode": "report_fields", "ids": ["FBgn0000055"], "fields": ["name"], "target_classes": ["FBgn"]}),
        json!({"mode": "record_cross_references", "ids": ["Fbxl4"], "fields": ["name"]}),
        json!({"mode": "record_cross_references", "ids": ["Fbxl4"], "target_classes": ["FBzz"]}),
        json!({"mode": "record_cross_references", "ids": ["Fbxl4"], "target_classes": ["FBmc"]}),
        json!({"mode": "record_cross_references", "ids": ["Fbxl4", "bad\ttoken"], "limit": 1}),
    ] {
        assert_eq!(call_batch(&server, args).await["error"], "invalid_input");
    }
}

#[test]
fn wide_json_and_tsv_pages_resume_at_first_unreturned_input() {
    let fixture = Fixture::new();
    let catalogs = fixture.load();
    let mut ids = vec!["FBst0000002"];
    ids.extend(["FBgn0004002"; 8]);
    for tsv in [false, true] {
        let first = page(&catalogs, &ids, 0, 0, &[], tsv);
        assert_eq!(first["truncated"], false);
        assert!(serde_json::to_vec(&first).unwrap().len() <= MAX_JSON_BYTES);
        let offset = first["next_offset"].as_u64().unwrap() as usize;
        assert!((2..ids.len()).contains(&offset));
        assert!(first["next_link_offset"].is_null());
        assert_eq!(first["columns"], json!(["FBal", "FBba"]));
        let second = page(&catalogs, &ids, offset, 0, &[], tsv);
        assert_eq!(second["offset"], offset);
        assert_eq!(second["columns"], json!(["FBal"]));
        assert!(second["next_offset"].is_null());
        if tsv {
            let header = "input\tid\tstatus\tlink_offset\tFBal\tFBba";
            assert_eq!(first["tsv"].as_str().unwrap().lines().next(), Some(header));
            assert_eq!(first["row_count"], offset);
            assert_eq!(second["row_count"], ids.len() - offset);
        } else {
            assert_eq!(first["rows"].as_array().unwrap().len(), offset);
            assert_eq!(second["rows"].as_array().unwrap().len(), ids.len() - offset);
        }
    }
}
