-- $1 FBal, $2 fetch, $3 offset. Allele-level genetic interactions from phenotype_comparison: the genotype carrying the allele,
-- the genotype it is compared with, the phenotype, the comparison outcome (enhanceable, suppressible, ...), and the reference.
with gts as (
  select distinct fg.genotype_id from feature a join feature_genotype fg on fg.feature_id = a.feature_id where a.uniquename = $1
)
select g1.uniquename as genotype, g2.uniquename as compared_with,
  coalesce(nullif(obs.name, 'unspecified'), cval.name) as phenotype,
  (select string_agg(c.name, '; ' order by pcc.rank) from phenotype_comparison_cvterm pcc join cvterm c on c.cvterm_id = pcc.cvterm_id where pcc.phenotype_comparison_id = pc.phenotype_comparison_id) as outcome,
  pub.uniquename as reference, pub.miniref
from gts join phenotype_comparison pc on pc.genotype1_id = gts.genotype_id
join genotype g1 on g1.genotype_id = pc.genotype1_id join genotype g2 on g2.genotype_id = pc.genotype2_id
left join phenotype p on p.phenotype_id = pc.phenotype1_id
left join cvterm obs on obs.cvterm_id = p.observable_id left join cvterm cval on cval.cvterm_id = p.cvalue_id
join pub on pub.pub_id = pc.pub_id
where g1.uniquename not like 'OBSOLETE%' and g2.uniquename not like 'OBSOLETE%'
order by g1.uniquename, g2.uniquename, pub.uniquename limit $2 offset $3
