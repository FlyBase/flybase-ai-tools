select a.fbal_id as id, array_to_string((array_agg(distinct c.fbtp_id order by c.fbtp_id))[1:50], '|') as value, count(distinct c.fbtp_id) as n
from gene.allele a join gene.construct c on c.allele_id = a.id where a.fbal_id = any($1::text[]) group by a.fbal_id
