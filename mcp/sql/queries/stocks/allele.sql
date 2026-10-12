-- $1 FBal, $2 gal4_only, $3 provider or null, $4 fetch, $5 offset. Stocks carrying the allele; gal4_only keeps stocks that
-- also carry a Scer\GAL4 allele (gene.allele_stock joined back by stock).
select distinct on (s.fbst_id) s.fbst_id as id, s.center as provider, s.stock_number, s.genotype
from gene.allele a join gene.allele_stock s on s.allele_id = a.id
where a.fbal_id = $1 and ($3::text is null or s.center = $3)
  and (not $2::bool or exists (select 1 from gene.allele_stock s2 join gene.allele a2 on a2.id = s2.allele_id where s2.fbst_id = s.fbst_id and left(a2.symbol, 9) = 'Scer\GAL4'))
order by s.fbst_id limit $4 offset $5
