with ph as (
  select a.feature_id as allele_id,
         case when obs.name = 'unspecified' then cval.cvterm_id else obs.cvterm_id end as term_id
  from feature g join feature_relationship ar on ar.object_id = g.feature_id join cvterm art on art.cvterm_id = ar.type_id and art.name = 'alleleof'
  join feature a on a.feature_id = ar.subject_id and a.is_obsolete = false
  join feature_genotype fg on fg.feature_id = a.feature_id join phenstatement ps on ps.genotype_id = fg.genotype_id
  join phenotype p on p.phenotype_id = ps.phenotype_id
  left join cvterm obs on obs.cvterm_id = p.observable_id left join cvterm cval on cval.cvterm_id = p.cvalue_id
  where g.uniquename = $1
)
select t.name as term, (select db.name || ':' || d.accession from dbxref d join db on db.db_id = d.db_id where d.dbxref_id = t.dbxref_id) as term_id,
       count(distinct ph.allele_id) as allele_count
from ph join cvterm t on t.cvterm_id = ph.term_id
where t.name <> 'unspecified'
group by t.name, t.dbxref_id order by allele_count desc, t.name
limit $2
