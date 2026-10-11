-- $1 cvterm_id, $2 include descendants, $3 species or null, $4 fetch, $5 offset. `hits` = 200001 means the cap was reached and the tool refuses.
-- Genes annotated to the term (or its descendants) through feature_cvterm, with evidence codes and reference count.
-- `matched` (the raw annotation rows) is materialized on its own, and `hits` looks each row's feature up through a
-- LATERAL subquery: a plain join on `feature` (32M rows) made the planner hash-join sequential scans of both tables
-- (9 to 12 s), while the correlated subquery is always a primary-key probe. The gene, obsolete, and species filters sit
-- inside `hits` before the 200,001-row cap, so the cap counts only annotations the page can return. When the page is
-- empty a sentinel row (null id) still carries the hit count so the tool can tell a capped term from an empty page.
with terms as (select $1::int as cvterm_id union select descendant_id from mcp.cvterm_closure where $2::bool and ancestor_id = $1),
matched as materialized (
  select fc.feature_cvterm_id, fc.feature_id, fc.cvterm_id, fc.pub_id from feature_cvterm fc join terms on terms.cvterm_id = fc.cvterm_id where fc.is_not = false),
hits as materialized (
  select m.feature_cvterm_id, m.feature_id, m.cvterm_id, m.pub_id, f.uniquename, f.name, f.type_id, f.species
  from matched m cross join lateral (
    select f.uniquename, f.name, f.type_id, o.abbreviation as species from feature f join organism o on o.organism_id = f.organism_id
    where f.feature_id = m.feature_id and f.is_obsolete = false and left(f.uniquename, 4) = 'FBgn' and ($3::text is null or o.abbreviation = $3) limit 1) f
  limit 200001),
page as (
  select a.uniquename as id, (select count(*) from hits) as hits, a.name as symbol, ft.name as type, a.species,
    count(distinct a.pub_id) as reference_count, count(distinct a.cvterm_id) as terms,
    (select string_agg(distinct p.value, '; ') from feature_cvtermprop p join cvterm pt on pt.cvterm_id = p.type_id
       join hits a2 on a2.feature_cvterm_id = p.feature_cvterm_id where a2.feature_id = a.feature_id and pt.name = 'evidence_code') as evidence
  from hits a join cvterm ft on ft.cvterm_id = a.type_id
  group by a.feature_id, a.uniquename, a.name, ft.name, a.species
  order by reference_count desc, a.name limit $4 offset $5)
select * from page
union all select null, (select count(*) from hits), null, null, null, null, null, null where not exists (select 1 from page)
