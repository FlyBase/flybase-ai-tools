-- $1 FBgn or FBal. Split-system combinations (FBco) built from alleles of the gene (or from the allele), with their stock counts.
select distinct c.fbco_id as id, c.symbol, c.stocks_count
from gene.split_system_combination c join gene.split_system_combination_component_allele ca on ca.split_system_combination_id = c.id
join gene.allele a on a.id = ca.allele_id join feature g on g.feature_id = a.gene_id
where ($1 = g.uniquename or $1 = a.fbal_id) order by c.fbco_id limit 200
