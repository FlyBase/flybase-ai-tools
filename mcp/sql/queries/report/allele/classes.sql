select c.name, c.fbcv_id from gene.allele a join gene.allele_class c on c.allele_id = a.id where a.fbal_id = $1 order by c.name
