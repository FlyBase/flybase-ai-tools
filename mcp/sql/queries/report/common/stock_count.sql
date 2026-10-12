-- $1 uniquename of an insertion, aberration, balancer, or construct: stocks from the derived_stock_* properties.
select count(distinct fbst)::bigint as n from flybase.get_stocks($1)
