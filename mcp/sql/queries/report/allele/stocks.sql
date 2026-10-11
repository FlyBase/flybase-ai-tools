select s.fbst_id as id, s.center as provider, s.stock_number, s.genotype from gene.allele_stock s join gene.allele a on a.id = s.allele_id
where a.fbal_id = $1 order by s.center, s.stock_number limit $2
