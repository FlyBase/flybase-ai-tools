select n.entity_id, n.data_class, n.species, n.name, n.name_kind, n.is_current,
       f.name as current_symbol, similarity(n.name, $1) as sim
from mcp.name_index n
left join feature f on f.uniquename = n.entity_id
where (lower(n.name) = lower($1) or n.name % $1 or lower(n.name) like lower($1) || '%')
  and ($2::text is null or n.species = $2)
  and ($3::text is null or n.data_class = $3)
order by (lower(n.name) = lower($1)) desc, n.is_current desc, sim desc
limit 200
