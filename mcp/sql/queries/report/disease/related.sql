-- $1 FBhh. Related disease model entries in either direction.
select x.relationship, x.direction, x.id, x.name from (
  select rt.name as relationship, 'subject' as direction, o.uniquename as id, o.name
  from humanhealth h join humanhealth_relationship r on r.subject_id = h.humanhealth_id join cvterm rt on rt.cvterm_id = r.type_id
  join humanhealth o on o.humanhealth_id = r.object_id and o.is_obsolete = false where h.uniquename = $1
  union all
  select rt.name, 'object', s.uniquename, s.name
  from humanhealth h join humanhealth_relationship r on r.object_id = h.humanhealth_id join cvterm rt on rt.cvterm_id = r.type_id
  join humanhealth s on s.humanhealth_id = r.subject_id and s.is_obsolete = false where h.uniquename = $1
) x order by x.direction, x.relationship, x.id limit 200
