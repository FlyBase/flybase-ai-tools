select i.fbti_id as id, i.symbol from gene.insertion i join gene.allele a on a.id = i.allele_id where a.fbal_id = $1 order by i.fbti_id limit $2
