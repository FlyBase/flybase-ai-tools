-- $1 FBgn, $2 gal4_only, $3 provider or null, $4 fetch, $5 offset. Stocks carrying an allele of the gene, including alleles that
-- use the gene's regulatory region (GAL4 and other drivers), one row per stock. gal4_only keeps only stocks matched through a
-- Scer\GAL4 allele (a GAL4 driver built from this gene's regulatory region).
select distinct on (s.fbst_id) s.fbst_id as id, s.center as provider, s.stock_number, s.genotype, a.fbal_id as allele_id, a.symbol as allele_symbol,
  a.gene_is_regulatory_region as via_regulatory_region
from feature g join gene.allele a on a.gene_id = g.feature_id join gene.allele_stock s on s.allele_id = a.id
where g.uniquename = $1 and (not $2::bool or (a.gene_is_regulatory_region and left(a.symbol, 9) = 'Scer\GAL4')) and ($3::text is null or s.center = $3)
order by s.fbst_id, a.is_alleleof desc, a.fbal_id
limit $4 offset $5
