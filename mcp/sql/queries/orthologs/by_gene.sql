-- $1 FBgn, $2 species abbreviations or null for all. Orthologs from orthologous_to relationships (the gene is the subject).
-- fr.value carries the DIOPT detail block for non-Drosophila species ("diopt methods:\t...\ndiopt best_score:\tYes\n...");
-- Drosophila pairs come from OrthoDB and have no DIOPT block. Human score and OMIM phenotypes join from
-- mcp.diopt_orthologs (migration 006) with gene_human_orthologv2 as the fallback for the score.
select t.uniquename as ortholog_id, t.name as symbol, o.abbreviation as species, fr.value as detail,
  (select d.accession from feature_dbxref fd join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id
     where fd.feature_id = t.feature_id and fd.is_current and db.name = 'HGNC' order by d.accession limit 1) as hgnc,
  coalesce(dio.score, hv.diopt_score) as human_score, dio.omim_phenotypes
from feature g join feature_relationship fr on fr.subject_id = g.feature_id
join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'orthologous_to'
join feature t on t.feature_id = fr.object_id and t.is_obsolete = false join organism o on o.organism_id = t.organism_id
left join dataclass_relationship.gene_human_orthologv2 hv on hv.gene_id = g.uniquename and hv.ortholog_id = t.uniquename
left join mcp.diopt_orthologs dio on dio.dmel_gene_id = g.uniquename and o.abbreviation = 'Hsap'
  and dio.hgnc_id = (select 'HGNC:' || d.accession from feature_dbxref fd join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id
     where fd.feature_id = t.feature_id and fd.is_current and db.name = 'HGNC' order by d.accession limit 1)
where g.uniquename = $1 and ($2::text[] is null or o.abbreviation = any($2::text[]))
order by o.abbreviation, t.name limit 1000
