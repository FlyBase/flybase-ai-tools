/// Named SQL loaded at compile time. Add one arm per query file.
pub fn get(name: &str) -> &'static str {
    match name {
        "resolve/candidates" => include_str!("../../sql/queries/resolve/candidates.sql"),
        "resolve/id_status" => include_str!("../../sql/queries/resolve/id_status.sql"),
        "species/list" => include_str!("../../sql/queries/species/list.sql"),
        "startup/migrations" => include_str!("../../sql/queries/startup/migrations.sql"),
        "startup/ping" => include_str!("../../sql/queries/startup/ping.sql"),
        "startup/tables" => include_str!("../../sql/queries/startup/tables.sql"),
        other => panic!("unknown query {other}"),
    }
}
