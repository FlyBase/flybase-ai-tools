/// Named SQL loaded at compile time. Add one arm per query file.
pub fn get(name: &str) -> &'static str {
    match name {
        "resolve/candidates" => include_str!("../../sql/queries/resolve/candidates.sql"),
        "resolve/id_status" => include_str!("../../sql/queries/resolve/id_status.sql"),
        "search/names" => include_str!("../../sql/queries/search/names.sql"),
        "search/references" => include_str!("../../sql/queries/search/references.sql"),
        "search/terms" => include_str!("../../sql/queries/search/terms.sql"),
        "species/list" => include_str!("../../sql/queries/species/list.sql"),
        "species/normalize" => include_str!("../../sql/queries/species/normalize.sql"),
        "startup/migrations" => include_str!("../../sql/queries/startup/migrations.sql"),
        "startup/ping" => include_str!("../../sql/queries/startup/ping.sql"),
        "startup/tables" => include_str!("../../sql/queries/startup/tables.sql"),
        other => panic!("unknown query {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::get;
    use std::path::Path;

    fn sql_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                sql_files(&p, out);
            } else if p.extension().is_some_and(|x| x == "sql") {
                out.push(p);
            }
        }
    }

    /// Every query file on disk is registered under its path-derived key, with its exact contents.
    #[test]
    fn registry_covers_every_query_file() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("sql/queries");
        let mut files = Vec::new();
        sql_files(&root, &mut files);
        assert!(files.len() >= 10, "found {} query files", files.len());
        for f in files {
            let key = f
                .strip_prefix(&root)
                .unwrap()
                .with_extension("")
                .to_string_lossy()
                .into_owned();
            let on_disk = std::fs::read_to_string(&f).unwrap();
            assert_eq!(get(&key), on_disk, "registry entry for {key} is stale");
        }
    }
}
