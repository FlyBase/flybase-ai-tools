-- $1 FBgn, $2 gal4_only, $3 provider or null. Total distinct stocks for the gene under the same filters as stocks/gene.
select count(distinct s.fbst_id)::bigint as n
from feature g join gene.allele a on a.gene_id = g.feature_id join gene.allele_stock s on s.allele_id = a.id
where g.uniquename = $1 and (not $2::bool or left(a.symbol, 9) = 'Scer\GAL4') and ($3::text is null or s.center = $3)
