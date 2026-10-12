select c.fbtp_id as id, c.symbol from gene.construct c join gene.allele a on a.id = c.allele_id where a.fbal_id = $1 order by c.fbtp_id limit $2
