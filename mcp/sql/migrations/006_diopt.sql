-- Human orthologs with DIOPT scores and OMIM phenotypes, loaded by `flybase-mcp migrate --diopt <file>` from the
-- release file precomputed_files/orthologs/dmel_human_orthologs_disease_fb_<release>.tsv(.gz). The table is created
-- empty here so queries compile before the file is loaded; get_orthologs falls back to gene_human_orthologv2 for scores.
create table if not exists mcp.diopt_orthologs (
  dmel_gene_id text not null, dmel_symbol text, hgnc_id text not null, omim_gene_id text, human_symbol text,
  score integer, omim_phenotype_ids text, omim_phenotypes text,
  primary key (dmel_gene_id, hgnc_id));
create index if not exists diopt_orthologs_hgnc on mcp.diopt_orthologs (hgnc_id);
