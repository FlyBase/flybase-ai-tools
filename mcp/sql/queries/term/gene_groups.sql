-- $1 cvterm_id, $2 include descendants, $3 fetch, $4 offset. Gene groups and pathways annotated to the term.
with terms as (select $1::int as cvterm_id union select descendant_id from mcp.cvterm_closure where $2::bool and ancestor_id = $1)
select g.uniquename as id, g.name as symbol, count(distinct gc.cvterm_id) as terms
from grp_cvterm gc join terms on terms.cvterm_id = gc.cvterm_id join grp g on g.grp_id = gc.grp_id and g.is_obsolete = false
where gc.is_not = false group by g.grp_id, g.uniquename, g.name order by g.name limit $3 offset $4
