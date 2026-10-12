select a.fbal_id as id, array_to_string((array_agg(distinct m.name order by m.name))[1:50], '|') as value, count(distinct m.name) as n
from gene.allele a join gene.allele_mutagen m on m.allele_id = a.id where a.fbal_id = any($1::text[]) group by a.fbal_id
