select m.name, m.fbcv_id from gene.allele a join gene.allele_mutagen m on m.allele_id = a.id where a.fbal_id = $1 order by m.name
