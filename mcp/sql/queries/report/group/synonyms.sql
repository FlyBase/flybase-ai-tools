-- $1 FBgg.
select distinct s.name, st.name as kind, gs.is_current
from grp g join grp_synonym gs on gs.grp_id = g.grp_id and gs.is_internal = false join synonym s on s.synonym_id = gs.synonym_id join cvterm st on st.cvterm_id = s.type_id
where g.uniquename = $1 order by gs.is_current desc, st.name, s.name limit 200
