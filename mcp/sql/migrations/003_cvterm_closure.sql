drop table if exists mcp.cvterm_closure;
create table mcp.cvterm_closure as
with recursive rel as (
  select r.subject_id as descendant_id, r.object_id as ancestor_id, 1 as depth
  from cvterm_relationship r join cvterm t on t.cvterm_id = r.type_id where t.name in ('is_a', 'part_of')
  union
  select rel.descendant_id, r.object_id, rel.depth + 1
  from rel join cvterm_relationship r on r.subject_id = rel.ancestor_id
  join cvterm t on t.cvterm_id = r.type_id where t.name in ('is_a', 'part_of') and rel.depth < 40
)
select rel.ancestor_id, rel.descendant_id, min(rel.depth) as depth, cv.name as cv
from rel join cvterm a on a.cvterm_id = rel.ancestor_id join cv on cv.cv_id = a.cv_id
group by rel.ancestor_id, rel.descendant_id, cv.name;
create index cvterm_closure_anc on mcp.cvterm_closure (ancestor_id);
create index cvterm_closure_desc on mcp.cvterm_closure (descendant_id);
create index if not exists cvterm_name_trgm on cvterm using gin (name gin_trgm_ops);
create index if not exists cvtermsynonym_name_trgm on cvtermsynonym using gin (name gin_trgm_ops);
create index if not exists pub_title_trgm on pub using gin (title gin_trgm_ops);
analyze mcp.cvterm_closure;
