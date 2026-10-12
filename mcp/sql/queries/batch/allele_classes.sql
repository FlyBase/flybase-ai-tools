select a.fbal_id as id, array_to_string((array_agg(distinct c.name order by c.name))[1:50], '|') as value, count(distinct c.name) as n
from gene.allele a join gene.allele_class c on c.allele_id = a.id where a.fbal_id = any($1::text[]) group by a.fbal_id
