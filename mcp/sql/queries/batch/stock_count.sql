select g.uniquename as id, count(distinct s.fbst_id)::text as value, 1::bigint as n
from feature g join gene.allele a on a.gene_id = g.feature_id join gene.allele_stock s on s.allele_id = a.id
where g.uniquename = any($1::text[]) group by g.uniquename
