-- $1 FBgn, $2 species abbreviations or null for all, $3 minimum score or null, $4 best matches only.
-- Orthologs from orthologous_to relationships (the gene is the subject). fr.value carries the DIOPT detail block for
-- non-Drosophila species ("diopt methods:\t...\ndiopt best_score:\tYes\n..."); Drosophila pairs come from OrthoDB
-- and have no DIOPT block. The score is the human file's (mcp.diopt_orthologs, migration 006), else
-- gene_human_orthologv2, else the number of DIOPT methods in the block. The HGNC accession comes from
-- gene_human_orthologv2 (99.6% of Dmel-Hsap pairs, always agreeing with the HGNC dbxref) and otherwise from one
-- dbxref lookup per human ortholog (about 650 dbxrefs per human feature, so it is the expensive path).
-- Filtered and ordered here so the cap (1001 rows: the caller reports 1,000 and flags the rest) never drops a
-- species of a gene with many orthologs.
with pairs as (
  select t.uniquename as ortholog_id, t.name as symbol, o.abbreviation as species, fr.value as detail,
    coalesce(hv.accession, h.accession) as hgnc,
    coalesce(dio.score, hv.diopt_score,
             array_length(string_to_array(substring(fr.value from 'diopt methods:\t([^\n]*)'), ','), 1)) as score,
    dio.omim_phenotypes,
    (fr.value ~* 'diopt best_score:[ \t]*yes' or fr.value ~* 'diopt best_score_rev:[ \t]*yes') as best
  from feature g join feature_relationship fr on fr.subject_id = g.feature_id
  join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'orthologous_to'
  join feature t on t.feature_id = fr.object_id and t.is_obsolete = false join organism o on o.organism_id = t.organism_id
  left join dataclass_relationship.gene_human_orthologv2 hv on hv.gene_id = g.uniquename and hv.ortholog_id = t.uniquename
  left join lateral (select d.accession from feature_dbxref fd join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id
                     where o.abbreviation = 'Hsap' and hv.accession is null
                       and fd.feature_id = t.feature_id and fd.is_current and db.name = 'HGNC'
                     order by d.accession limit 1) h on true
  left join mcp.diopt_orthologs dio on dio.dmel_gene_id = g.uniquename and dio.hgnc_id = 'HGNC:' || coalesce(hv.accession, h.accession)
  where g.uniquename = $1 and ($2::text[] is null or o.abbreviation = any($2::text[]))
)
select ortholog_id, symbol, species, detail, hgnc, score, omim_phenotypes
from pairs
where ($3::int is null or score >= $3::int) and (not $4::bool or best)
order by score desc nulls last, species, symbol limit 1001
