-- $1 uniquename, $2 fetch. Stocks from the derived_stock_* properties (insertions, aberrations, balancers, constructs).
select distinct s.fbst as id, s.center as provider, s.stock_number, s.genotype
from flybase.get_stocks($1) s order by s.center, s.stock_number limit $2
