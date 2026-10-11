-- $1 FBgg. Parent groups (this group is the subject of parent_grp) and child groups (this group is the object).
select x.relationship, x.direction, x.id, x.symbol, x.name from (
  select rt.name as relationship, 'parent' as direction, o.uniquename as id, o.name as symbol,
    (select s.name from grp_synonym gs join synonym s on s.synonym_id = gs.synonym_id join cvterm st on st.cvterm_id = s.type_id where gs.grp_id = o.grp_id and gs.is_current and st.name = 'fullname' limit 1) as name
  from grp g join grp_relationship r on r.subject_id = g.grp_id join cvterm rt on rt.cvterm_id = r.type_id join grp o on o.grp_id = r.object_id and o.is_obsolete = false
  where g.uniquename = $1
  union all
  select rt.name, 'child', s.uniquename, s.name,
    (select sy.name from grp_synonym gs join synonym sy on sy.synonym_id = gs.synonym_id join cvterm st on st.cvterm_id = sy.type_id where gs.grp_id = s.grp_id and gs.is_current and st.name = 'fullname' limit 1)
  from grp g join grp_relationship r on r.object_id = g.grp_id join cvterm rt on rt.cvterm_id = r.type_id join grp s on s.grp_id = r.subject_id and s.is_obsolete = false
  where g.uniquename = $1
) x order by x.direction, x.relationship, x.id limit 200
