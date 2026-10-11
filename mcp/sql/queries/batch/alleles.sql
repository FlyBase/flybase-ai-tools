select g.uniquename as id, array_to_string((array_agg(a.fbal_id order by a.fbal_id))[1:50], '|') as value, count(*) as n
from feature g join gene.allele a on a.gene_id = g.feature_id
where g.uniquename = any($1::text[]) group by g.uniquename
