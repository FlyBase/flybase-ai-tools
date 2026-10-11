-- $1 text[] of symbols, names, or stock numbers, $2 species or null. Exact, case-insensitive matches of current symbols,
-- full names, and stock-centre numbers in the name index.
select x.input, n.entity_id, n.data_class, n.species, n.name as matched, n.name_kind, f.name as current_symbol
from unnest($1::text[]) x(input)
join mcp.name_index n on lower(n.name) = lower(x.input) and n.is_current and n.name_kind in ('symbol', 'fullname', 'stock_number')
left join feature f on f.uniquename = n.entity_id
where $2::text is null or n.species = $2
order by x.input, (n.name_kind = 'symbol') desc, (n.species = 'Dmel') desc, n.entity_id
limit 5000
