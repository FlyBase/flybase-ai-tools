select coalesce(sum(a.stocks_count), 0)::bigint as n from gene.allele a join feature g on g.feature_id = a.gene_id where g.uniquename = $1
