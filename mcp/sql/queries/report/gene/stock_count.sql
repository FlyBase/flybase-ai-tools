-- $1 FBgn. Distinct stocks across the gene's alleles (a stock carrying two alleles of the gene counts once).
select count(distinct s.fbst_id)::bigint as n
from gene.allele a join feature g on g.feature_id = a.gene_id join gene.allele_stock s on s.allele_id = a.id
where g.uniquename = $1
