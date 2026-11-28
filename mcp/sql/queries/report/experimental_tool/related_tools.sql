-- $1 FBto, $2 fetch. Compatible and other tools come from explicit links in either
-- direction. Common gene of origin derives from two originates_from relationships.
with tool as (select feature_id from feature where uniquename = $1),
direct as (
  select fr.feature_relationship_id, fr.type_id, fr.object_id as partner_id
  from tool f join feature_relationship fr on fr.subject_id = f.feature_id
  union all
  select fr.feature_relationship_id, fr.type_id, fr.subject_id
  from tool f join feature_relationship fr on fr.object_id = f.feature_id
),
tool_links as (
  select d.partner_id, t.name as relationship, d.feature_relationship_id
  from direct d join cvterm t on t.cvterm_id = d.type_id
  join feature p on p.feature_id = d.partner_id and not p.is_obsolete and not p.is_analysis
  where p.uniquename like 'FBto%' and p.uniquename <> $1
),
shared_origins as (
  select p.feature_id as partner_id, g.uniquename as origin_gene_id, g.name as origin_gene_symbol,
         fr.feature_relationship_id as tool_relationship_id, other.feature_relationship_id as partner_relationship_id
  from tool f join feature_relationship fr on fr.subject_id = f.feature_id
  join cvterm t on t.cvterm_id = fr.type_id and t.name = 'originates_from'
  join feature g on g.feature_id = fr.object_id and g.uniquename like 'FBgn%'
    and not g.is_obsolete and not g.is_analysis
  join feature_relationship other on other.object_id = g.feature_id and other.type_id = fr.type_id
  join feature p on p.feature_id = other.subject_id and p.uniquename like 'FBto%'
    and not p.is_obsolete and not p.is_analysis and p.feature_id <> f.feature_id
),
edges as (
  select l.partner_id, case when l.relationship = 'compatible_tool' then 'compatible' else 'other' end as category,
         l.relationship, null::text as origin_gene_id, null::text as origin_gene_symbol,
         l.feature_relationship_id
  from tool_links l
  where l.relationship = 'compatible_tool'
     or (not exists (select from tool_links c where c.partner_id = l.partner_id and c.relationship = 'compatible_tool')
     and not exists (select from shared_origins s where s.partner_id = l.partner_id))
  union all
  select s.partner_id, 'common_gene_origin', 'originates_from', s.origin_gene_id, s.origin_gene_symbol, ids.relationship_id
  from shared_origins s
  cross join lateral (values (s.tool_relationship_id), (s.partner_relationship_id)) ids(relationship_id)
),
page as (
  select e.partner_id, f.uniquename as id, f.name as symbol, e.category, e.origin_gene_id, e.origin_gene_symbol,
         array_agg(distinct e.relationship order by e.relationship) as relationships,
         array_agg(distinct e.feature_relationship_id) as relationship_ids
  from edges e join feature f on f.feature_id = e.partner_id
  group by e.partner_id, f.uniquename, f.name, e.category, e.origin_gene_id, e.origin_gene_symbol
  order by case e.category when 'compatible' then 1 when 'other' then 2 else 3 end, f.name, f.uniquename, e.origin_gene_id
  limit $2
)
select page.*,
  array(select distinct p.uniquename
        from feature_relationship_pub frp join pub p using (pub_id)
        where frp.feature_relationship_id = any(page.relationship_ids) and p.uniquename ~ '^FBrf[0-9]+$'
        order by p.uniquename) as reference_ids,
  array(select distinct db.name || ':' || dx.accession
        from feature_cvterm fc join cvterm t using (cvterm_id)
        join dbxref dx on dx.dbxref_id = t.dbxref_id join db using (db_id)
        where fc.feature_id = page.partner_id and not fc.is_not
          and exists (select from feature_cvtermprop cp join cvterm pt on pt.cvterm_id = cp.type_id
                      where cp.feature_cvterm_id = fc.feature_cvterm_id and pt.name = 'tool_uses')
        order by 1) as use_ids
from page
order by case category when 'compatible' then 1 when 'other' then 2 else 3 end, symbol, id, origin_gene_id
