-- $1 raw query, $2 species or null, $3 data_class or null, $4 fetch count, $5 offset, $6 prefix_only,
-- $7 query with LIKE wildcards escaped by backslash.
-- Three or more characters use the GIN trigram index through ilike; two-character queries use the
-- lower(name) text_pattern_ops btree through the prefix branch.
select n.entity_id, n.data_class, n.species, n.name as matched, n.name_kind, f.name as current_symbol,
       case when lower(n.name) = lower($1) then 'exact'
            when lower(n.name) like (lower($7) || '%') escape '\' then 'prefix'
            else 'substring' end as match_kind
from mcp.name_index n left join feature f on f.uniquename = n.entity_id
where (($6::bool and lower(n.name) like (lower($7) || '%') escape '\')
       or (not $6::bool and n.name ilike ('%' || $7 || '%') escape '\'))
  and ($2::text is null or n.species = $2)
  and ($3::text is null or n.data_class = $3)
order by (lower(n.name) = lower($1)) desc, (lower(n.name) like (lower($7) || '%') escape '\') desc,
         n.is_current desc, length(n.name), n.entity_id
limit $4 offset $5
