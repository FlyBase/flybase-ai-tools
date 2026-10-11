/// Named SQL loaded at compile time. Add one arm per query file.
pub fn get(name: &str) -> &'static str {
    match name {
        "report/allele/classes" => include_str!("../../sql/queries/report/allele/classes.sql"),
        "report/allele/constructs" => {
            include_str!("../../sql/queries/report/allele/constructs.sql")
        }
        "report/allele/gene" => include_str!("../../sql/queries/report/allele/gene.sql"),
        "report/allele/identity" => include_str!("../../sql/queries/report/allele/identity.sql"),
        "report/allele/insertions" => {
            include_str!("../../sql/queries/report/allele/insertions.sql")
        }
        "report/allele/mutagens" => include_str!("../../sql/queries/report/allele/mutagens.sql"),
        "report/allele/phenotype_summary" => {
            include_str!("../../sql/queries/report/allele/phenotype_summary.sql")
        }
        "report/allele/phenotypes" => {
            include_str!("../../sql/queries/report/allele/phenotypes.sql")
        }
        "report/allele/stock_count" => {
            include_str!("../../sql/queries/report/allele/stock_count.sql")
        }
        "report/allele/stocks" => include_str!("../../sql/queries/report/allele/stocks.sql"),
        "report/allele/transgenic_product_class" => {
            include_str!("../../sql/queries/report/allele/transgenic_product_class.sql")
        }
        "report/common/references" => {
            include_str!("../../sql/queries/report/common/references.sql")
        }
        "report/common/synonyms" => include_str!("../../sql/queries/report/common/synonyms.sql"),
        "report/gene/alleles" => include_str!("../../sql/queries/report/gene/alleles.sql"),
        "report/gene/external_links" => {
            include_str!("../../sql/queries/report/gene/external_links.sql")
        }
        "report/gene/go" => include_str!("../../sql/queries/report/gene/go.sql"),
        "report/gene/identity" => include_str!("../../sql/queries/report/gene/identity.sql"),
        "report/gene/location" => include_str!("../../sql/queries/report/gene/location.sql"),
        "report/gene/ortholog_count" => {
            include_str!("../../sql/queries/report/gene/ortholog_count.sql")
        }
        "report/gene/phenotype_summary" => {
            include_str!("../../sql/queries/report/gene/phenotype_summary.sql")
        }
        "report/gene/snapshot" => include_str!("../../sql/queries/report/gene/snapshot.sql"),
        "report/gene/stock_count" => include_str!("../../sql/queries/report/gene/stock_count.sql"),
        "report/gene/transcripts" => include_str!("../../sql/queries/report/gene/transcripts.sql"),
        "report/reference/abstract" => {
            include_str!("../../sql/queries/report/reference/abstract.sql")
        }
        "report/reference/alleles" => {
            include_str!("../../sql/queries/report/reference/alleles.sql")
        }
        "report/reference/authors" => {
            include_str!("../../sql/queries/report/reference/authors.sql")
        }
        "report/reference/citation" => {
            include_str!("../../sql/queries/report/reference/citation.sql")
        }
        "report/reference/data_types" => {
            include_str!("../../sql/queries/report/reference/data_types.sql")
        }
        "report/reference/genes" => include_str!("../../sql/queries/report/reference/genes.sql"),
        "report/stock/components" => include_str!("../../sql/queries/report/stock/components.sql"),
        "report/stock/identity" => include_str!("../../sql/queries/report/stock/identity.sql"),
        "resolve/candidates" => include_str!("../../sql/queries/resolve/candidates.sql"),
        "resolve/id_status" => include_str!("../../sql/queries/resolve/id_status.sql"),
        "search/names" => include_str!("../../sql/queries/search/names.sql"),
        "search/references" => include_str!("../../sql/queries/search/references.sql"),
        "search/terms" => include_str!("../../sql/queries/search/terms.sql"),
        "sequence/feature_residues" => {
            include_str!("../../sql/queries/sequence/feature_residues.sql")
        }
        "sequence/gene_location" => include_str!("../../sql/queries/sequence/gene_location.sql"),
        "sequence/gene_transcripts" => {
            include_str!("../../sql/queries/sequence/gene_transcripts.sql")
        }
        "sequence/region" => include_str!("../../sql/queries/sequence/region.sql"),
        "sequence/transcript_parts" => {
            include_str!("../../sql/queries/sequence/transcript_parts.sql")
        }
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
