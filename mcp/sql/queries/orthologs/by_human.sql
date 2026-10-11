-- $1 FBog of a human gene. Drosophila melanogaster genes it is orthologous to, with the DIOPT detail block.
select g.uniquename as gene_id, g.name as symbol, fr.value as detail, coalesce(dio.score, hv.diopt_score) as human_score, dio.omim_phenotypes
from feature h join feature_relationship fr on fr.subject_id = h.feature_id
join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'orthologous_to'
join feature g on g.feature_id = fr.object_id and g.is_obsolete = false join organism o on o.organism_id = g.organism_id and o.abbreviation = 'Dmel'
left join dataclass_relationship.gene_human_orthologv2 hv on hv.gene_id = g.uniquename and hv.ortholog_id = h.uniquename
left join mcp.diopt_orthologs dio on dio.dmel_gene_id = g.uniquename and dio.hgnc_id = (select 'HGNC:' || d.accession from feature_dbxref fd join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id
     where fd.feature_id = h.feature_id and fd.is_current and db.name = 'HGNC' order by d.accession limit 1)
where h.uniquename = $1 order by g.name limit 1000
