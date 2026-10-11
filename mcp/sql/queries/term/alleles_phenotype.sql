-- $1 cvterm_id, $2 include descendants, $3 species or null, $4 fetch, $5 offset. `hits` = 200001 means the cap was reached and the tool refuses.
-- Alleles with a phenotype whose observable (anatomy) or phenotypic class is the term, with statement and reference counts.
-- `statements` counts distinct phenstatement rows: feature_genotype lists an allele once per copy, so a homozygote
-- would otherwise count each statement twice.
-- The observable and phenotypic-class matches are two equality joins unioned (an IN over both columns defeats both indexes).
-- `matched` (the statement rows) is materialized on its own; `hits` probes `feature` through a LATERAL subquery with the
-- allele, obsolete, and species filters inside it before the 200,001-row cap; the empty-page sentinel row is as in term/genes.sql.
with terms as (select $1::int as cvterm_id union select descendant_id from mcp.cvterm_closure where $2::bool and ancestor_id = $1),
ph as (select p.phenotype_id from phenotype p join terms on terms.cvterm_id = p.observable_id
       union select p.phenotype_id from phenotype p join terms on terms.cvterm_id = p.cvalue_id),
matched as materialized (
  select fg.feature_id, ps.pub_id, ps.phenstatement_id from ph join phenstatement ps on ps.phenotype_id = ph.phenotype_id join feature_genotype fg on fg.genotype_id = ps.genotype_id),
hits as materialized (
  select m.feature_id, m.pub_id, m.phenstatement_id, a.uniquename, a.name, a.species
  from matched m cross join lateral (
    select a.uniquename, a.name, o.abbreviation as species from feature a join organism o on o.organism_id = a.organism_id
    where a.feature_id = m.feature_id and a.is_obsolete = false and left(a.uniquename, 4) = 'FBal' and ($3::text is null or o.abbreviation = $3) limit 1) a
  limit 200001),
page as (
  select a.uniquename as id, (select count(*) from hits) as hits, a.name as symbol, a.species, count(distinct a.pub_id) as reference_count, count(distinct a.phenstatement_id) as statements
  from hits a
  group by a.feature_id, a.uniquename, a.name, a.species
  order by reference_count desc, a.name limit $4 offset $5)
select * from page
union all select null, (select count(*) from hits), null, null, null, null where not exists (select 1 from page)
