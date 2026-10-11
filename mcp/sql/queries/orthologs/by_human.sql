-- $1 FBog of a human gene, $2 minimum score or null, $3 best matches only. Drosophila melanogaster genes it is
-- orthologous to, with the DIOPT detail block; score, filtering, and ordering as in by_gene.sql. The human gene's
-- HGNC accession is looked up once.
with hgnc as (
  select d.accession from feature h join feature_dbxref fd on fd.feature_id = h.feature_id and fd.is_current
  join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id and db.name = 'HGNC'
  where h.uniquename = $1 order by d.accession limit 1
), pairs as (
  select g.uniquename as gene_id, g.name as symbol, fr.value as detail,
    coalesce(dio.score, hv.diopt_score,
             array_length(string_to_array(substring(fr.value from 'diopt methods:\t([^\n]*)'), ','), 1)) as score,
    dio.omim_phenotypes,
    (fr.value ~* 'diopt best_score:[ \t]*yes' or fr.value ~* 'diopt best_score_rev:[ \t]*yes') as best
  from feature h join feature_relationship fr on fr.subject_id = h.feature_id
  join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'orthologous_to'
  join feature g on g.feature_id = fr.object_id and g.is_obsolete = false join organism o on o.organism_id = g.organism_id and o.abbreviation = 'Dmel'
  left join dataclass_relationship.gene_human_orthologv2 hv on hv.gene_id = g.uniquename and hv.ortholog_id = h.uniquename
  left join mcp.diopt_orthologs dio on dio.dmel_gene_id = g.uniquename and dio.hgnc_id = (select 'HGNC:' || accession from hgnc)
  where h.uniquename = $1
)
select gene_id, symbol, detail, score, omim_phenotypes
from pairs
where ($2::int is null or score >= $2::int) and (not $3::bool or best)
order by score desc nulls last, symbol limit 1001
