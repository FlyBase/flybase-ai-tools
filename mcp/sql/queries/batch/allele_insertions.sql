select a.fbal_id as id, array_to_string((array_agg(distinct i.fbti_id order by i.fbti_id))[1:50], '|') as value, count(distinct i.fbti_id) as n
from gene.allele a join gene.insertion i on i.allele_id = a.id where a.fbal_id = any($1::text[]) group by a.fbal_id
