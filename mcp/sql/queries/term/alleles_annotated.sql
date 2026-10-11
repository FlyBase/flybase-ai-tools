-- $1 cvterm_id, $2 include descendants, $3 species or null, $4 fetch, $5 offset. `hits` = 200001 means the cap was reached and the tool refuses.
-- Alleles annotated to the term (or its descendants) through feature_cvterm (Disease Ontology models), with qualifiers, evidence, and reference count.
-- Materialized CTEs as in term/genes.sql; the 200,001-row cap bounds the planner's estimate so it looks features up by key.
with terms as (select $1::int as cvterm_id union select descendant_id from mcp.cvterm_closure where $2::bool and ancestor_id = $1),
hits as materialized (
  select fc.feature_cvterm_id, fc.feature_id, fc.cvterm_id, fc.pub_id from feature_cvterm fc join terms on terms.cvterm_id = fc.cvterm_id limit 200001),
ann as materialized (
  select h.feature_cvterm_id, h.feature_id, h.cvterm_id, h.pub_id, f.uniquename, f.name, f.organism_id
  from hits h join feature f on f.feature_id = h.feature_id and f.is_obsolete = false and left(f.uniquename, 4) = 'FBal')
select a.uniquename as id, (select count(*) from hits) as hits, a.name as symbol, o.abbreviation as species, count(distinct a.pub_id) as reference_count, count(distinct a.cvterm_id) as terms,
  (select string_agg(distinct p.value, '; ') from feature_cvtermprop p join cvterm pt on pt.cvterm_id = p.type_id
     join ann a2 on a2.feature_cvterm_id = p.feature_cvterm_id where a2.feature_id = a.feature_id and pt.name = 'qualifier') as qualifiers,
  (select string_agg(distinct p.value, '; ') from feature_cvtermprop p join cvterm pt on pt.cvterm_id = p.type_id
     join ann a2 on a2.feature_cvterm_id = p.feature_cvterm_id where a2.feature_id = a.feature_id and pt.name = 'evidence_code') as evidence
from ann a join organism o on o.organism_id = a.organism_id
where $3::text is null or o.abbreviation = $3
group by a.feature_id, a.uniquename, a.name, o.abbreviation
order by reference_count desc, a.name limit $4 offset $5
