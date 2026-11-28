-- $1 FBig. The group description is the release's ordered list of interacting gene IDs.
select array(
  select distinct t.name
  from interaction_group_feature_interaction j
  join feature_interaction fi using (feature_interaction_id)
  join interaction i using (interaction_id)
  join cvterm t on t.cvterm_id = i.type_id
  where j.interaction_group_id = g.interaction_group_id and not i.is_obsolete
  order by t.name
) as interaction_types, coalesce(genes.items, '[]'::jsonb) as genes
from interaction_group g
left join lateral (
  select jsonb_agg(jsonb_build_object('id', member.id, 'symbol', f.name) order by member.position) as items
  from unnest(string_to_array(g.description, '-')) with ordinality as member(id, position)
  left join feature f on f.uniquename = member.id and not f.is_obsolete
) genes on true
where g.uniquename = $1 and not g.is_obsolete
