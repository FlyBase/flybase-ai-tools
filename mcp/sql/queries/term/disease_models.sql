-- $1 cvterm_id, $2 include descendants, $3 fetch, $4 offset. Human disease model entries annotated to the DO term.
with terms as (select $1::int as cvterm_id union select descendant_id from mcp.cvterm_closure where $2::bool and ancestor_id = $1)
select h.uniquename as id, h.name, count(distinct hc.cvterm_id) as terms
from humanhealth_cvterm hc join terms on terms.cvterm_id = hc.cvterm_id join humanhealth h on h.humanhealth_id = hc.humanhealth_id and h.is_obsolete = false
group by h.humanhealth_id, h.uniquename, h.name order by h.name limit $3 offset $4
