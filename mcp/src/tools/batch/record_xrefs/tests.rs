use super::*;
use rusqlite::Connection;
use sha2::{Digest, Sha256};
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
            format!("# release: {RELEASE}\nFBlc0000001\nFBst0000002\nFBst0000004\nFBtc0000012\n"),
        )
        .unwrap();

        let maps = Self::index(
            root.join("maps.sqlite"),
            json!({
                "symbols": {"rows": 3},
                "symbol_ids": {"rows": 2},
            }),
        );
        maps.execute_batch(
            "create table maps(source text,id text,value text,primary key(source,id));\
             insert into maps values\
               ('symbols','FBgn0000055','Adh'),\
               ('symbols','FBgn0004000','dense source'),\
               ('symbols','FBgn0004001','no relationships'),\
               ('symbol_ids','Adh','FBgn0000055'),\
               ('symbol_ids','alpha[example]','FBgn0000055');",
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
        let mut sources = serde_json::Map::new();
        for (name, contents, records) in [
            ("general", general.as_str(), 2),
            (
                "references",
                "FBrf0015431\tFBgn0000055\n\t\tFBst0000002\n",
                1,
            ),
            (
                "stock_objects",
                "FBst0000002\tFBal0000021\nFBst0000002\tFBal0005583\nFBst0000002\tFBba0000025\nFBst0000002\tFBsv:0000101\nFBst0000002\tFBsn0000277\n",
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
        ids[offset..]
            .iter()
            .map(|id| input_token(id).unwrap())
            .collect(),
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
        " fbGN0000055 ",
        "&agr;<up>example</up>",
        "FBst0000002",
        "FBrf0015431",
        "FBst0000004",
        "FBrf9999999",
        "FBst0000001",
        "Adh",
        "FBgn0004001",
        "adh",
    ];
    let result = page(&catalogs, &ids, 0, 0, &[], false);
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), ids.len());
    assert_eq!(rows[0]["input"], "FBgn0000055");
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
    for index in [4, 5, 8] {
        assert_eq!(rows[index]["status"], "resolved");
        assert_eq!(rows[index]["cross_references"], json!({}));
    }
    for index in [6, 9] {
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
        "input\tid\tstatus\tlink_offset\tFBba\nFBst0000002\tFBst0000002\tresolved\t0\tFBba0000025\n'=1+1\t\tunknown\t0\t\n",
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
    assert!(RecordXrefs::load(&fixture.0, RELEASE).is_err());
}
