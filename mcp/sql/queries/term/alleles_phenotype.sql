-- $1 cvterm_id, $2 include descendants, $3 species or null, $4 fetch, $5 offset. `hits` = 200001 means the cap was reached and the tool refuses.
-- Alleles with a phenotype whose observable (anatomy) or phenotypic class is the term, with statement and reference counts.
-- The observable and phenotypic-class matches are two equality joins unioned (an IN over both columns defeats both indexes),
-- and the statement rows are materialized with a 200,001-row cap so the planner looks alleles up by key instead of
-- scanning `feature`.
with terms as (select $1::int as cvterm_id union select descendant_id from mcp.cvterm_closure where $2::bool and ancestor_id = $1),
ph as (select p.phenotype_id from phenotype p join terms on terms.cvterm_id = p.observable_id
       union select p.phenotype_id from phenotype p join terms on terms.cvterm_id = p.cvalue_id),
st as materialized (
  select fg.feature_id, ps.pub_id from ph join phenstatement ps on ps.phenotype_id = ph.phenotype_id join feature_genotype fg on fg.genotype_id = ps.genotype_id limit 200001),
ann as materialized (
  select st.feature_id, st.pub_id, a.uniquename, a.name, a.organism_id
  from st join feature a on a.feature_id = st.feature_id and a.is_obsolete = false and left(a.uniquename, 4) = 'FBal')
select a.uniquename as id, (select count(*) from st) as hits, a.name as symbol, o.abbreviation as species, count(distinct a.pub_id) as reference_count, count(*) as statements
from ann a join organism o on o.organism_id = a.organism_id
where $3::text is null or o.abbreviation = $3
group by a.feature_id, a.uniquename, a.name, o.abbreviation
order by reference_count desc, a.name limit $4 offset $5
