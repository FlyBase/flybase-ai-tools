select g.uniquename as genotype, coalesce(nullif(obs.name, 'unspecified'), cval.name) as phenotype,
  (select string_agg(ct.name, '; ') from phenotype_cvterm pc join cvterm ct on ct.cvterm_id = pc.cvterm_id where pc.phenotype_id = p.phenotype_id) as qualifiers,
  pub.uniquename as reference, pub.miniref
from feature a join feature_genotype fg on fg.feature_id = a.feature_id join genotype g on g.genotype_id = fg.genotype_id
join phenstatement ps on ps.genotype_id = g.genotype_id join phenotype p on p.phenotype_id = ps.phenotype_id
left join cvterm obs on obs.cvterm_id = p.observable_id left join cvterm cval on cval.cvterm_id = p.cvalue_id
join pub on pub.pub_id = ps.pub_id
where a.uniquename = $1 and g.uniquename not like 'OBSOLETE%'
order by phenotype, g.uniquename limit $2
