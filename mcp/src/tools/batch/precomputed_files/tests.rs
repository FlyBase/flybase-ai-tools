use super::*;
use crate::tools::batch::record_xrefs::tests::{
    Fixture as RecordFixture, batch_server, call_batch,
};
use flate2::{Compression, write::ZlibEncoder};
use rusqlite::{Connection, params};
use std::{io::Write, sync::Arc};

const RELEASE: &str = "FB2026_03";
const ADH: &str = "FBgn0000055";
const DPP: &str = "FBgn0000490";

fn fixture() -> Connection {
    let db = Connection::open_in_memory().unwrap();
    db.execute_batch(
        "CREATE TABLE metadata(key TEXT PRIMARY KEY, value TEXT NOT NULL);
         INSERT INTO metadata VALUES('schema','1'),('release','FB2026_03'),('complete','true');
         CREATE TABLE files(id INTEGER PRIMARY KEY, key TEXT UNIQUE, label TEXT, path TEXT,
                            classes TEXT, line_count INTEGER, sha256 TEXT);
         CREATE TABLE blocks(file INTEGER, block INTEGER, data BLOB, PRIMARY KEY(file,block)) WITHOUT ROWID;
         CREATE TABLE matches(file INTEGER, id TEXT, block INTEGER, PRIMARY KEY(file,id,block)) WITHOUT ROWID;
         CREATE TABLE aliases(key BLOB PRIMARY KEY, value BLOB) WITHOUT ROWID;
         CREATE TABLE external(kind TEXT, key TEXT, id TEXT, PRIMARY KEY(kind,key,id)) WITHOUT ROWID;
         INSERT INTO external VALUES ('accession','NP_123','FBgn0000055');",
    )
    .unwrap();
    for (key, value) in [("syn adh", ADH), ("syn Adh", DPP), ("sec FBgn0000055", DPP)] {
        db.execute(
            "INSERT INTO aliases VALUES(?,?)",
            params![key.as_bytes(), value.as_bytes()],
        )
        .unwrap();
    }
    db
}

fn add_file(db: &Connection, id: i64, key: &str, path: &str, blocks: &[(&str, &[&str])]) {
    let line_count: usize = blocks
        .iter()
        .map(|(rows, _)| rows.split_inclusive('\n').count())
        .sum();
    db.execute(
        "INSERT INTO files VALUES(?,?,?,?,?,?,?)",
        params![
            id,
            key,
            key,
            path,
            "[\"FBgn\"]",
            line_count as i64,
            "a".repeat(64)
        ],
    )
    .unwrap();
    for (block, (text, matches)) in blocks.iter().enumerate() {
        let mut out = ZlibEncoder::new(Vec::new(), Compression::fast());
        out.write_all(text.as_bytes()).unwrap();
        db.execute(
            "INSERT INTO blocks VALUES(?,?,?)",
            params![id, block as i64, out.finish().unwrap()],
        )
        .unwrap();
        for token in *matches {
            db.execute(
                "INSERT INTO matches VALUES(?,?,?)",
                params![id, token, block as i64],
            )
            .unwrap();
        }
    }
}

#[tokio::test]
async fn filtered_source_order_across_blocks_and_files() {
    let db = fixture();
    let first_block = format!(
        "# file header\nnot-{ADH}\nlabel\t{ADH}\r\n{}{DPP}\t{ADH}\n",
        "skip\n".repeat(252)
    );
    add_file(
        &db,
        1,
        "genes",
        "genes/genes.tsv.gz",
        &[(&first_block, &["", ADH, DPP]), ("# footer", &[""])],
    );
    add_file(
        &db,
        2,
        "physical_interactions",
        "genes/physical_interactions_mitab_fb_2026_03.tsv.gz",
        &[(
            &format!("x\tflybase:{ADH}\tmitab\nx\tx\tflybase:{DPP}\n"),
            &[ADH],
        )],
    );
    let mut server = batch_server().await;
    let options = json!({"mode":"precomputed_files","list_options":true});
    assert_eq!(
        call_batch(&server, options.clone()).await["available"],
        false
    );
    let report_options = call_batch(&server, json!({"list_options":true})).await;
    assert!(
        report_options["modes"]
            .as_array()
            .unwrap()
            .contains(&json!("precomputed_files"))
    );
    let mut args = json!({"mode":"precomputed_files", "files":["genes","physical_interactions"], "ids":[DPP,ADH,ADH], "limit":1});
    assert_eq!(
        call_batch(&server, args.clone()).await["error"],
        "backend_unavailable"
    );
    server.precomputed_files = Some(Arc::new(
        PrecomputedFiles::from_connection(db, RELEASE).unwrap(),
    ));
    let catalog = call_batch(&server, options).await;
    assert_eq!(catalog["available"], true);
    assert_eq!(catalog["files"][0]["key"], "genes");
    assert_eq!(catalog["files"][0]["classes"], json!(["FBgn"]));
    assert_eq!(
        catalog["files"][0]["url"],
        "https://s3ftp.flybase.org/releases/FB2026_03/precomputed_files/genes/genes.tsv.gz"
    );

    let expected = [
        ("genes", "# file header\n".to_owned(), Some(2)),
        ("genes", format!("label\t{ADH}\r\n"), Some(255)),
        ("genes", format!("{DPP}\t{ADH}\n"), Some(256)),
        ("genes", "# footer".to_owned(), Some(257)),
        (
            "physical_interactions",
            format!("x\tflybase:{ADH}\tmitab\n"),
            None,
        ),
    ];
    for format in ["json", "tsv"] {
        args["format"] = json!(format);
        args["offset"] = json!(0);
        for (file, line, next) in &expected {
            let page = call_batch(&server, args.clone()).await;
            assert_eq!(page["row_count"], 1, "{page}");
            assert_eq!(page["truncated"], false);
            assert_eq!(page["next_offset"], json!(next));
            if format == "tsv" {
                assert_eq!(page["tsv"], *line);
            } else {
                assert_eq!(page["rows"], json!([{"file": file, "line": line}]));
            }
            args["offset"] = page["next_offset"].clone();
        }
    }
    let unmatched = call_batch(
        &server,
        json!({
            "mode":"precomputed_files", "files":["genes"], "ids":["FBgn9999999"], "format":"tsv"
        }),
    )
    .await;
    assert_eq!(unmatched["tsv"], "# file header\n# footer");
    assert!(unmatched["next_offset"].is_null());
}

#[tokio::test]
async fn exact_symbol_before_alias_and_literal_id_before_both() {
    let db = fixture();
    add_file(
        &db,
        1,
        "genes",
        "genes/genes.tsv.gz",
        &[(&format!("{ADH}\tx\n{DPP}\ty\n"), &[ADH, DPP])],
    );
    let mut server = batch_server().await;
    server.precomputed_files = Some(Arc::new(
        PrecomputedFiles::from_connection(db, RELEASE).unwrap(),
    ));
    let args = |ids: &[&str]| json!({"mode":"precomputed_files","files":["genes"],"format":"tsv","ids":ids});
    assert_eq!(
        call_batch(&server, args(&["Adh"])).await["error"],
        "backend_unavailable"
    );
    let records = RecordFixture::new();
    server.record_xrefs = Some(Arc::new(records.load()));
    for input in ["Adh", "adh", ADH, "NP_123.78"] {
        let page = call_batch(&server, args(&[input, "made-up"])).await;
        assert_eq!(page["tsv"], format!("{ADH}\tx\n"), "{input}");
        assert_eq!(page["unresolved_count"], 1);
    }
    assert_eq!(
        call_batch(&server, args(&["made-up"])).await["error"],
        "invalid_input"
    );
    for bad in [
        json!({"mode":"precomputed_files","files":["missing"],"ids":[ADH]}),
        json!({"mode":"precomputed_files","files":["genes","genes"],"ids":[ADH]}),
        json!({"mode":"precomputed_files","files":["genes"],"ids":[ADH],"offset":2}),
        json!({"mode":"precomputed_files","files":["genes"],"ids":[ADH],"fields":["symbol"]}),
        json!({"mode":"precomputed_files","files":["genes"],"ids":[ADH],"target_classes":["FBgn"]}),
        json!({"mode":"precomputed_files","files":["genes"],"ids":[ADH],"link_offset":0}),
        json!({"files":["genes"],"ids":[ADH]}),
        json!({"mode":"record_cross_references","files":["genes"],"ids":[ADH]}),
    ] {
        assert_eq!(call_batch(&server, bad).await["error"], "invalid_input");
    }
}

#[test]
fn wide_multipage_results_resume_at_next_source_line() {
    let db = fixture();
    let line = format!("{ADH}\t{}\n", "x".repeat(100_000));
    let huge = format!("{ADH}\t{}\n", "x".repeat(MAX_JSON_BYTES));
    add_file(
        &db,
        1,
        "genes",
        "genes/genes.tsv.gz",
        &[(&line.repeat(3), &[ADH])],
    );
    add_file(&db, 2, "huge", "genes/huge.tsv.gz", &[(&huge, &[ADH])]);
    let index = PrecomputedFiles::from_connection(db, RELEASE).unwrap();
    let ids = HashSet::from([ADH.to_owned()]);
    for format in ["json", "tsv"] {
        let mut args: BatchParams =
            serde_json::from_value(json!({"files":["genes"],"format":format})).unwrap();
        let files = select_files(&index, &args.files).unwrap();
        let first = read_page(&index, &files, &ids, &args, RELEASE, 0)
            .unwrap()
            .structured_content
            .unwrap();
        assert_eq!(first["next_offset"], 2);
        assert_eq!(first["row_count"], 2);
        assert_eq!(first["truncated"], false);
        assert!(serde_json::to_vec(&first).unwrap().len() <= MAX_JSON_BYTES);
        args.offset = Some(2);
        let last = read_page(&index, &files, &ids, &args, RELEASE, 0)
            .unwrap()
            .structured_content
            .unwrap();
        assert_eq!(last["row_count"], 1);
        assert!(last["next_offset"].is_null());
        args.files = vec!["huge".into()];
        args.offset = None;
        let huge = select_files(&index, &args.files).unwrap();
        assert_eq!(
            read_page(&index, &huge, &ids, &args, RELEASE, 0)
                .unwrap_err()
                .code(),
            "limit_exceeded"
        );
    }
}

#[test]
fn incompatible_or_corrupt_indexes_fail_closed() {
    assert!(PrecomputedFiles::from_connection(fixture(), "FB2026_01").is_err());
    let db = fixture();
    add_file(
        &db,
        1,
        "genes",
        "genes/genes.tsv.gz",
        &[("test line\n", &[""])],
    );
    db.execute("UPDATE blocks SET data=X'ff' WHERE file=1", [])
        .unwrap();
    let index = PrecomputedFiles::from_connection(db, RELEASE).unwrap();
    let files = select_files(&index, &["genes".into()]).unwrap();
    let args: BatchParams = serde_json::from_value(json!({"files":["genes"]})).unwrap();
    let ids = HashSet::from([ADH.to_owned()]);
    assert_eq!(
        read_page(&index, &files, &ids, &args, RELEASE, 0)
            .unwrap_err()
            .code(),
        "backend_unavailable"
    );
}

#[tokio::test]
async fn fifty_thousand_distinct_ids_fit_one_request() {
    let db = fixture();
    add_file(
        &db,
        1,
        "genes",
        "genes/genes.tsv.gz",
        &[(&format!("{ADH}\tx\n{DPP}\ty\n"), &[ADH, DPP])],
    );
    let mut server = batch_server().await;
    server.precomputed_files = Some(Arc::new(
        PrecomputedFiles::from_connection(db, RELEASE).unwrap(),
    ));
    let mut ids: Vec<_> = (0..50_000)
        .map(|number| format!("FBgn{number:07}"))
        .collect();
    let mut args =
        json!({"mode":"precomputed_files", "files":["genes"], "ids":ids, "format":"tsv"});
    let page = call_batch(&server, args.clone()).await;
    assert_eq!(page["tsv"], format!("{ADH}\tx\n{DPP}\ty\n"));
    assert!(page["next_offset"].is_null());
    ids.push(ADH.into());
    args["ids"] = json!(ids);
    assert_eq!(call_batch(&server, args).await["error"], "limit_exceeded");
}
