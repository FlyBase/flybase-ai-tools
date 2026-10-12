select a.fbal_id, a.symbol, a.is_construct, a.stocks_count, a.paper_count,
  (select string_agg(c.name, '; ') from gene.allele_class c where c.allele_id = a.id) as classes
from gene.allele a join feature g on g.feature_id = a.gene_id
where g.uniquename = $1 order by a.paper_count desc nulls last, a.fbal_id limit $2
