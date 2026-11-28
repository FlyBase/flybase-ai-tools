-- $1 FBto, $2 fetch. An insertion belongs here when its parent FBtp has a direct
-- relationship to the tool. An annotation on an allele alone is insufficient.
with constructs as materialized (
  select distinct c.feature_id, c.uniquename, c.name
  from feature f
  join feature_relationship annotation on annotation.object_id = f.feature_id
  join feature c on c.feature_id = annotation.subject_id and c.uniquename like 'FBtp%'
    and not c.is_obsolete and not c.is_analysis
  where f.uniquename = $1
), page as (
  select i.feature_id, i.uniquename as id, i.name as symbol,
         c.feature_id as construct_feature_id, c.uniquename as construct_id, c.name as construct_symbol
  from constructs c
  join feature_relationship produced on produced.object_id = c.feature_id
  join feature i on i.feature_id = produced.subject_id and i.uniquename like 'FBti%'
    and not i.is_obsolete and not i.is_analysis
  where produced.type_id = any(array(select cvterm_id from cvterm where name = 'producedby'))
  group by i.feature_id, i.uniquename, i.name, c.feature_id, c.uniquename, c.name
  order by i.uniquename, c.uniquename limit $2
)
select page.*,
  array(
    select distinct a.uniquename
    from feature_relationship assoc join cvterm t on t.cvterm_id = assoc.type_id and t.name = 'associated_with'
    join feature a on a.feature_id = assoc.subject_id and a.uniquename like 'FBal%'
      and not a.is_obsolete and not a.is_analysis
    where assoc.object_id = page.feature_id order by a.uniquename
  ) as associated_allele_ids,
  -- Parallel arrays use the same order, with a null reference when a
  -- relationship has no citation; the Rust mapper groups these rows by role.
  annotations.relationships, annotations.reference_ids,
  components.encoded_tool_ids, components.tag_ids, components.carried_tool_ids,
  (select count(distinct fbst) from flybase.get_stocks(page.id)) as stock_count
from page
cross join lateral (
  select array_agg(x.relationship order by x.relationship, x.reference) as relationships,
         array_agg(x.reference order by x.relationship, x.reference) as reference_ids
  from (
    select distinct t.name as relationship, p.uniquename as reference
    from feature_relationship fr join cvterm t on t.cvterm_id = fr.type_id
    join feature f on f.feature_id = fr.object_id and f.uniquename = $1
    left join feature_relationship_pub pp using (feature_relationship_id)
    left join pub p on p.pub_id = pp.pub_id and p.uniquename ~ '^FBrf[0-9]+$'
    where fr.subject_id = page.construct_feature_id
  ) x
) annotations
cross join lateral (
  select coalesce(array_agg(distinct f.uniquename order by f.uniquename)
           filter (where t.name = 'encodes_tool'), '{}') as encoded_tool_ids,
         coalesce(array_agg(distinct f.uniquename order by f.uniquename)
           filter (where t.name = 'tagged_with'), '{}') as tag_ids,
         coalesce(array_agg(distinct f.uniquename order by f.uniquename)
           filter (where t.name = 'carries_tool'), '{}') as carried_tool_ids
  from feature_relationship fr join cvterm t on t.cvterm_id = fr.type_id
  join feature f on f.feature_id = fr.object_id and f.uniquename like 'FBto%'
    and not f.is_obsolete and not f.is_analysis
  where fr.subject_id = page.construct_feature_id
    and t.name in ('encodes_tool', 'tagged_with', 'carries_tool')
) components
order by id, construct_id
