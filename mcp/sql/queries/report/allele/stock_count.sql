select coalesce(stocks_count, 0)::bigint as n from gene.allele where fbal_id = $1
