-- $1 cvterm_id, $2 include descendants. Annotated entities by class: features (by type) through feature_cvterm,
-- alleles through phenotype statements (observable or phenotypic class), disease models through humanhealth_cvterm,
-- gene groups through grp_cvterm, libraries through library_cvterm. `hits` is materialized so the planner looks the
-- annotated features up by primary key; the phenotype match is a UNION of two equality joins.
with terms as (
  select $1::int as cvterm_id
  union select descendant_id from mcp.cvterm_closure where $2::bool and ancestor_id = $1
),
hits as materialized (select distinct fc.feature_id from feature_cvterm fc join terms on terms.cvterm_id = fc.cvterm_id),
ph as (select p.phenotype_id from phenotype p join terms on terms.cvterm_id = p.observable_id
       union select p.phenotype_id from phenotype p join terms on terms.cvterm_id = p.cvalue_id)
select 'feature:' || ft.name as class, count(*)::bigint as n
from hits join feature f on f.feature_id = hits.feature_id and f.is_obsolete = false join cvterm ft on ft.cvterm_id = f.type_id
group by ft.name
union all
select 'allele:phenotype', count(distinct fg.feature_id)
from ph join phenstatement ps on ps.phenotype_id = ph.phenotype_id join feature_genotype fg on fg.genotype_id = ps.genotype_id
join feature a on a.feature_id = fg.feature_id and left(a.uniquename, 4) = 'FBal'
union all
select 'disease_model', count(distinct hc.humanhealth_id) from humanhealth_cvterm hc join terms on terms.cvterm_id = hc.cvterm_id
union all
select 'gene_group', count(distinct gc.grp_id) from grp_cvterm gc join terms on terms.cvterm_id = gc.cvterm_id
union all
select 'library', count(distinct lc.library_id) from library_cvterm lc join terms on terms.cvterm_id = lc.cvterm_id
order by n desc
