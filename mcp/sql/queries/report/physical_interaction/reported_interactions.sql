-- $1 FBig, $2 fetch. Page distinct interaction records before expanding their evidence.
with grouped_features as (
  select distinct fi.feature_interaction_id, fi.interaction_id, j.ftype
  from interaction_group g
  join interaction_group_feature_interaction j using (interaction_group_id)
  join feature_interaction fi using (feature_interaction_id)
  where g.uniquename = $1 and not g.is_obsolete
), records as (
  select i.*
  from (select distinct interaction_id from grouped_features) grouped
  join interaction i using (interaction_id)
  where not i.is_obsolete
  order by i.uniquename, i.interaction_id
  limit $2
)
select i.uniquename, t.name as interaction_type, i.description as molecule_pair,
  array(
    select distinct p.uniquename
    from interaction_pub ip join pub p using (pub_id)
    where ip.interaction_id = i.interaction_id and not p.is_obsolete
    order by p.uniquename
  ) as reference_ids,
  coalesce(assays.items, '[]'::jsonb) as assays,
  coalesce(participants.items, '[]'::jsonb) as participants,
  coalesce(comments.items, '[]'::jsonb) as comments,
  coalesce(cell_lines.items, '[]'::jsonb) as cell_lines,
  coalesce(collections.items, '[]'::jsonb) as collections,
  coalesce(sources.items, '[]'::jsonb) as sources
from records i
join cvterm t on t.cvterm_id = i.type_id
left join lateral (
  select jsonb_agg(jsonb_build_object('id', d.name || ':' || x.accession, 'name', c.name)
                   order by c.name, x.accession) as items
  from interaction_cvterm ic
  join cvterm c using (cvterm_id)
  join cv using (cv_id)
  join dbxref x using (dbxref_id)
  join db d using (db_id)
  where ic.interaction_id = i.interaction_id and cv.name = 'PSI-MI'
) assays on true
left join lateral (
  select jsonb_agg(jsonb_build_object(
    'feature_id', case when f.uniquename ~ '^FB[a-z]{2}[0-9]+$' then f.uniquename end,
    'name', f.name,
    'kind', props.kind,
    'molecule_type', gf.ftype,
    'role', role.name,
    'gene_ids', array(
      with parents as (
        select parent.feature_id, parent.uniquename
        from feature_relationship fr
        join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'associated_with'
        join feature parent on parent.feature_id = fr.object_id
        where fr.subject_id = f.feature_id and not parent.is_obsolete
      )
      select f.uniquename
      where f.uniquename ~ '^FBgn[0-9]{7}$' and not f.is_obsolete
      union
      select parent.uniquename from parents parent where parent.uniquename ~ '^FBgn[0-9]{7}$'
      union
      select g.uniquename
      from parents parent
      join feature_relationship fr on fr.subject_id = parent.feature_id
      join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'associated_with'
      join feature g on g.feature_id = fr.object_id
      where parent.uniquename !~ '^FBgn[0-9]{7}$'
        and g.uniquename ~ '^FBgn[0-9]{7}$' and not g.is_obsolete
      order by uniquename
    ),
    'reported_as', coalesce(props.reported_as, '{}'),
    'notes', coalesce(props.notes, '{}'),
    'coordinates', coalesce(props.coordinates, '{}')
  ) order by (gf.feature_interaction_id is null), fi.rank, f.uniquename, role.name, fi.feature_interaction_id) as items
  from feature_interaction fi
  join feature f using (feature_id)
  join cvterm role on role.cvterm_id = fi.role_id
  left join grouped_features gf using (feature_interaction_id)
  cross join lateral (
    select
      case
        when gf.feature_interaction_id is not null then 'participant'
        when bool_or(pt.name = 'interacting region') then 'subregion'
        when bool_or(pt.name in ('interacting isoform', 'interacting isolate')) then 'isoform'
        when bool_or(pt.name = 'experimental feature') then 'experimental_entity'
      end as kind,
      array_agg(fp.value order by fp.rank) filter (where pt.name = 'reported as' and fp.value is not null) as reported_as,
      array_agg(fp.value order by fp.rank) filter (where pt.name = 'comment' and fp.value is not null) as notes,
      array_agg(fp.value order by fp.rank) filter (where pt.name = 'subpart_info' and fp.value is not null) as coordinates
    from feature_interactionprop fp join cvterm pt on pt.cvterm_id = fp.type_id
    where fp.feature_interaction_id = fi.feature_interaction_id
  ) props
  where fi.interaction_id = i.interaction_id and props.kind is not null
) participants on true
left join lateral (
  select jsonb_agg(jsonb_build_object('kind', pt.name, 'text', ip.value, 'reference_ids', array(
    select p.uniquename
    from interactionprop_pub ipp join pub p using (pub_id)
    where ipp.interactionprop_id = ip.interactionprop_id and not p.is_obsolete
    order by p.uniquename
  )) order by ip.interactionprop_id) as items
  from interactionprop ip join cvterm pt on pt.cvterm_id = ip.type_id
  where ip.interaction_id = i.interaction_id and ip.value is not null and pt.name like '%comment%'
) comments on true
left join lateral (
  select jsonb_agg(jsonb_build_object('id', c.uniquename, 'name', c.name) order by c.uniquename) as items
  from cell_line c
  where not c.is_obsolete and c.cell_line_id in (
    select ic.cell_line_id from interaction_cell_line ic where ic.interaction_id = i.interaction_id
  )
) cell_lines on true
left join lateral (
  select jsonb_agg(jsonb_build_object('id', l.uniquename, 'name', l.name) order by l.uniquename) as items
  from library l
  where not l.is_obsolete and l.library_id in (
    select li.library_id from library_interaction li where li.interaction_id = i.interaction_id
  )
) collections on true
left join lateral (
  select jsonb_agg(jsonb_build_object('expression_id', e.uniquename, 'kind', et.name,
                                       'id', d.name || ':' || x.accession, 'name', c.name)
                   order by e.uniquename, et.name, ec.rank, c.name, x.accession) as items
  from expression e
  join expression_cvterm ec using (expression_id)
  join cvterm et on et.cvterm_id = ec.cvterm_type_id
  join cvterm c on c.cvterm_id = ec.cvterm_id
  join dbxref x on x.dbxref_id = c.dbxref_id
  join db d using (db_id)
  where et.name in ('anatomy', 'stage') and e.expression_id in (
    select ie.expression_id from interaction_expression ie where ie.interaction_id = i.interaction_id
  )
) sources on true
order by i.uniquename, i.interaction_id
