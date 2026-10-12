-- $1 uniquename, $2 fetch. Relationships in both directions with the other feature's id, symbol, and type.
select x.relationship, x.direction, x.other_id, x.other_symbol, x.other_type from (
  select rt.name as relationship, 'subject' as direction, t.uniquename as other_id, t.name as other_symbol, tt.name as other_type
  from feature f join feature_relationship fr on fr.subject_id = f.feature_id join cvterm rt on rt.cvterm_id = fr.type_id
  join feature t on t.feature_id = fr.object_id and t.is_obsolete = false join cvterm tt on tt.cvterm_id = t.type_id
  where f.uniquename = $1
  union all
  select rt.name, 'object', s.uniquename, s.name, st.name
  from feature f join feature_relationship fr on fr.object_id = f.feature_id join cvterm rt on rt.cvterm_id = fr.type_id
  join feature s on s.feature_id = fr.subject_id and s.is_obsolete = false join cvterm st on st.cvterm_id = s.type_id
  where f.uniquename = $1
) x order by x.direction, x.relationship, x.other_id limit $2
