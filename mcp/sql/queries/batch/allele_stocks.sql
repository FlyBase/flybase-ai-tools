select a.fbal_id as id, array_to_string((array_agg(distinct s.fbst_id order by s.fbst_id))[1:50], '|') as value, count(distinct s.fbst_id) as n
from gene.allele a join gene.allele_stock s on s.allele_id = a.id where a.fbal_id = any($1::text[]) group by a.fbal_id
