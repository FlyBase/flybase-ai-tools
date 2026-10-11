/// Named SQL loaded at compile time. Add one arm per query file.
pub fn get(name: &str) -> &'static str {
    match name {
        "batch/allele_classes" => include_str!("../../sql/queries/batch/allele_classes.sql"),
        "batch/allele_constructs" => include_str!("../../sql/queries/batch/allele_constructs.sql"),
        "batch/allele_gene" => include_str!("../../sql/queries/batch/allele_gene.sql"),
        "batch/allele_insertions" => include_str!("../../sql/queries/batch/allele_insertions.sql"),
        "batch/allele_mutagens" => include_str!("../../sql/queries/batch/allele_mutagens.sql"),
        "batch/allele_stocks" => include_str!("../../sql/queries/batch/allele_stocks.sql"),
        "batch/alleles" => include_str!("../../sql/queries/batch/alleles.sql"),
        "batch/annotation_id" => include_str!("../../sql/queries/batch/annotation_id.sql"),
        "batch/cytology" => include_str!("../../sql/queries/batch/cytology.sql"),
        "batch/external_ids" => include_str!("../../sql/queries/batch/external_ids.sql"),
        "batch/gene_type" => include_str!("../../sql/queries/batch/gene_type.sql"),
        "batch/go_terms" => include_str!("../../sql/queries/batch/go_terms.sql"),
        "batch/human_orthologs" => include_str!("../../sql/queries/batch/human_orthologs.sql"),
        "batch/location" => include_str!("../../sql/queries/batch/location.sql"),
        "batch/name" => include_str!("../../sql/queries/batch/name.sql"),
        "batch/polypeptides" => include_str!("../../sql/queries/batch/polypeptides.sql"),
        "batch/pubmed_ids" => include_str!("../../sql/queries/batch/pubmed_ids.sql"),
        "batch/references" => include_str!("../../sql/queries/batch/references.sql"),
        "batch/species" => include_str!("../../sql/queries/batch/species.sql"),
        "batch/stock_count" => include_str!("../../sql/queries/batch/stock_count.sql"),
        "batch/symbol" => include_str!("../../sql/queries/batch/symbol.sql"),
        "batch/synonyms" => include_str!("../../sql/queries/batch/synonyms.sql"),
        "batch/transcripts" => include_str!("../../sql/queries/batch/transcripts.sql"),
        "batch/transgenic_product_class" => {
            include_str!("../../sql/queries/batch/transgenic_product_class.sql")
        }
        "batch/type" => include_str!("../../sql/queries/batch/type.sql"),
        "disease/variants" => include_str!("../../sql/queries/disease/variants.sql"),
        "report/disease/do_terms" => include_str!("../../sql/queries/report/disease/do_terms.sql"),
        "report/disease/genes" => include_str!("../../sql/queries/report/disease/genes.sql"),
        "report/disease/identity" => include_str!("../../sql/queries/report/disease/identity.sql"),
        "report/disease/props" => include_str!("../../sql/queries/report/disease/props.sql"),
        "report/disease/references" => {
            include_str!("../../sql/queries/report/disease/references.sql")
        }
        "report/disease/related" => include_str!("../../sql/queries/report/disease/related.sql"),
        "report/disease/synonyms" => include_str!("../../sql/queries/report/disease/synonyms.sql"),
        "report/group/cvterms" => include_str!("../../sql/queries/report/group/cvterms.sql"),
        "report/group/hierarchy" => include_str!("../../sql/queries/report/group/hierarchy.sql"),
        "report/group/identity" => include_str!("../../sql/queries/report/group/identity.sql"),
        "report/group/members" => include_str!("../../sql/queries/report/group/members.sql"),
        "report/group/references" => include_str!("../../sql/queries/report/group/references.sql"),
        "report/group/synonyms" => include_str!("../../sql/queries/report/group/synonyms.sql"),
        "report/aberration/genes" => include_str!("../../sql/queries/report/aberration/genes.sql"),
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
        "report/balancer/components" => {
            include_str!("../../sql/queries/report/balancer/components.sql")
        }
        "report/cell_line/cvterms" => {
            include_str!("../../sql/queries/report/cell_line/cvterms.sql")
        }
        "report/cell_line/features" => {
            include_str!("../../sql/queries/report/cell_line/features.sql")
        }
        "report/cell_line/identity" => {
            include_str!("../../sql/queries/report/cell_line/identity.sql")
        }
        "report/cell_line/props" => include_str!("../../sql/queries/report/cell_line/props.sql"),
        "report/cell_line/references" => {
            include_str!("../../sql/queries/report/cell_line/references.sql")
        }
        "report/cell_line/synonyms" => {
            include_str!("../../sql/queries/report/cell_line/synonyms.sql")
        }
        "report/common/cvterms" => include_str!("../../sql/queries/report/common/cvterms.sql"),
        "report/common/feature_identity" => {
            include_str!("../../sql/queries/report/common/feature_identity.sql")
        }
        "report/common/location" => include_str!("../../sql/queries/report/common/location.sql"),
        "report/common/props" => include_str!("../../sql/queries/report/common/props.sql"),
        "report/common/props_all" => include_str!("../../sql/queries/report/common/props_all.sql"),
        "report/common/references" => {
            include_str!("../../sql/queries/report/common/references.sql")
        }
        "report/common/relationships" => {
            include_str!("../../sql/queries/report/common/relationships.sql")
        }
        "report/common/stock_count" => {
            include_str!("../../sql/queries/report/common/stock_count.sql")
        }
        "report/common/stocks" => include_str!("../../sql/queries/report/common/stocks.sql"),
        "report/common/synonyms" => include_str!("../../sql/queries/report/common/synonyms.sql"),
        "report/common/timeout_probe" => {
            include_str!("../../sql/queries/report/common/timeout_probe.sql")
        }
        "report/construct/allele_genes" => {
            include_str!("../../sql/queries/report/construct/allele_genes.sql")
        }
        "report/construct/components" => {
            include_str!("../../sql/queries/report/construct/components.sql")
        }
        "report/construct/insertions" => {
            include_str!("../../sql/queries/report/construct/insertions.sql")
        }
        "report/cvterm/annotation_counts" => {
            include_str!("../../sql/queries/report/cvterm/annotation_counts.sql")
        }
        "report/cvterm/children" => include_str!("../../sql/queries/report/cvterm/children.sql"),
        "report/cvterm/descendant_count" => {
            include_str!("../../sql/queries/report/cvterm/descendant_count.sql")
        }
        "report/cvterm/parents" => include_str!("../../sql/queries/report/cvterm/parents.sql"),
        "report/cvterm/synonyms" => include_str!("../../sql/queries/report/cvterm/synonyms.sql"),
        "report/cvterm/term" => include_str!("../../sql/queries/report/cvterm/term.sql"),
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
        "report/insertion/alleles" => {
            include_str!("../../sql/queries/report/insertion/alleles.sql")
        }
        "report/insertion/construct" => {
            include_str!("../../sql/queries/report/insertion/construct.sql")
        }
        "report/insertion/genes" => include_str!("../../sql/queries/report/insertion/genes.sql"),
        "report/library/cvterms" => include_str!("../../sql/queries/report/library/cvterms.sql"),
        "report/library/feature_count" => {
            include_str!("../../sql/queries/report/library/feature_count.sql")
        }
        "report/library/features" => include_str!("../../sql/queries/report/library/features.sql"),
        "report/library/identity" => include_str!("../../sql/queries/report/library/identity.sql"),
        "report/library/props" => include_str!("../../sql/queries/report/library/props.sql"),
        "report/library/references" => {
            include_str!("../../sql/queries/report/library/references.sql")
        }
        "report/library/synonyms" => include_str!("../../sql/queries/report/library/synonyms.sql"),
        "report/polypeptide/domains" => {
            include_str!("../../sql/queries/report/polypeptide/domains.sql")
        }
        "report/polypeptide/transcript" => {
            include_str!("../../sql/queries/report/polypeptide/transcript.sql")
        }
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
        "report/strain/identity" => include_str!("../../sql/queries/report/strain/identity.sql"),
        "report/strain/props" => include_str!("../../sql/queries/report/strain/props.sql"),
        "report/strain/references" => {
            include_str!("../../sql/queries/report/strain/references.sql")
        }
        "report/strain/synonyms" => include_str!("../../sql/queries/report/strain/synonyms.sql"),
        "report/transcript/gene" => include_str!("../../sql/queries/report/transcript/gene.sql"),
        "report/transcript/parts" => include_str!("../../sql/queries/report/transcript/parts.sql"),
        "report/transcript/polypeptide" => {
            include_str!("../../sql/queries/report/transcript/polypeptide.sql")
        }
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
        "vocab/search" => include_str!("../../sql/queries/vocab/search.sql"),
        "species/list" => include_str!("../../sql/queries/species/list.sql"),
        "species/normalize" => include_str!("../../sql/queries/species/normalize.sql"),
        "startup/functions" => include_str!("../../sql/queries/startup/functions.sql"),
        "startup/migrations" => include_str!("../../sql/queries/startup/migrations.sql"),
        "startup/ping" => include_str!("../../sql/queries/startup/ping.sql"),
        "startup/tables" => include_str!("../../sql/queries/startup/tables.sql"),
        "startup/timeout_setting" => {
            include_str!("../../sql/queries/startup/timeout_setting.sql")
        }
        "validate/external" => include_str!("../../sql/queries/validate/external.sql"),
        "validate/pubmed" => include_str!("../../sql/queries/validate/pubmed.sql"),
        "validate/symbols" => include_str!("../../sql/queries/validate/symbols.sql"),
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
