-- $1 FBto, $2 fetch. One row per tool role and annotated component with an
-- associated transgenic construct, engineered plasmid or molecular segment.
with page as (
  select roles.*, component.uniquename as component_id, component.name as component_symbol,
         constructs.ids as construct_ids
  from (
    select fr.subject_id as component_feature_id, role.name as relationship,
           array_agg(fr.feature_relationship_id) as relationship_ids
    from feature f join feature_relationship fr on fr.object_id = f.feature_id
    join cvterm role on role.cvterm_id = fr.type_id
      and role.name in ('encodes_tool', 'tagged_with', 'carries_tool', 'has_reg_region')
    where f.uniquename = $1
    group by fr.subject_id, role.name
    order by fr.subject_id, role.name
  ) roles
  -- Read the ordered role rows until the page is full. Keep these indexed
  -- lookups lateral so large tools do not expand every component first.
  cross join lateral (
    select f.uniquename, f.name from feature f
    where f.feature_id = roles.component_feature_id and not f.is_obsolete and not f.is_analysis
      and left(f.uniquename, 4) in ('FBal', 'FBtp', 'FBmc', 'FBms')
    limit 1
  ) component
  cross join lateral (
    select array_agg(distinct c.uniquename order by c.uniquename) as ids
    from feature_relationship assoc
    join feature c on c.feature_id = assoc.object_id and not c.is_obsolete and not c.is_analysis
      and left(c.uniquename, 4) in ('FBtp', 'FBmc', 'FBms')
    where assoc.subject_id = roles.component_feature_id
      and assoc.type_id = any(array(select cvterm_id from cvterm where name = 'associated_with'))
    having count(*) > 0
  ) constructs
  order by roles.component_feature_id, roles.relationship limit $2
)
select page.*,
  array(select distinct p.uniquename
        from feature_relationship_pub frp join pub p using (pub_id)
        where frp.feature_relationship_id = any(page.relationship_ids) and p.uniquename ~ '^FBrf[0-9]+$'
        order by p.uniquename) as reference_ids,
  components.regulatory_region_ids, components.tag_ids, components.carried_tool_ids,
  case when cardinality(components.encoded_tool_ids) > 0 then components.encoded_tool_ids
       else components.gene_ids end as encoded_product_ids,
  (select count(distinct fbst) from flybase.get_stocks(page.construct_ids)) as stock_count
from page
cross join lateral (
  select coalesce(array_agg(distinct f.uniquename order by f.uniquename)
           filter (where t.name = 'has_reg_region' and left(f.uniquename, 4) in ('FBgn', 'FBto')), '{}') as regulatory_region_ids,
         coalesce(array_agg(distinct f.uniquename order by f.uniquename)
           filter (where t.name = 'encodes_tool' and f.uniquename like 'FBto%'), '{}') as encoded_tool_ids,
         coalesce(array_agg(distinct f.uniquename order by f.uniquename)
           filter (where t.name = 'alleleof' and f.uniquename like 'FBgn%'), '{}') as gene_ids,
         coalesce(array_agg(distinct f.uniquename order by f.uniquename)
           filter (where t.name = 'tagged_with' and f.uniquename like 'FBto%'), '{}') as tag_ids,
         coalesce(array_agg(distinct f.uniquename order by f.uniquename)
           filter (where t.name = 'carries_tool' and f.uniquename like 'FBto%'), '{}') as carried_tool_ids
  from feature_relationship fr join cvterm t on t.cvterm_id = fr.type_id
  join feature f on f.feature_id = fr.object_id and not f.is_obsolete and not f.is_analysis
  where fr.subject_id = page.component_feature_id
    and t.name in ('has_reg_region', 'encodes_tool', 'alleleof', 'tagged_with', 'carries_tool')
) components
order by component_feature_id, relationship
