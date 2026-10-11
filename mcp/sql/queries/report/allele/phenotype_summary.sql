with gts as (
  select distinct g.genotype_id from feature a join feature_genotype fg on fg.feature_id = a.feature_id join genotype g on g.genotype_id = fg.genotype_id
  where a.uniquename = $1 and g.uniquename not like 'OBSOLETE%'
), ph as (
  select case when obs.name = 'unspecified' then cval.cvterm_id else obs.cvterm_id end as term_id
  from gts join phenstatement ps on ps.genotype_id = gts.genotype_id
  join phenotype p on p.phenotype_id = ps.phenotype_id
  left join cvterm obs on obs.cvterm_id = p.observable_id left join cvterm cval on cval.cvterm_id = p.cvalue_id
)
select t.name as term, (select db.name || ':' || d.accession from dbxref d join db on db.db_id = d.db_id where d.dbxref_id = t.dbxref_id) as term_id, count(*) as statements
from ph join cvterm t on t.cvterm_id = ph.term_id where t.name <> 'unspecified'
group by t.name, t.dbxref_id order by statements desc, t.name limit $2
