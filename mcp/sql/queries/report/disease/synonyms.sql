-- $1 FBhh.
select distinct s.name, st.name as kind, hs.is_current
from humanhealth h join humanhealth_synonym hs on hs.humanhealth_id = h.humanhealth_id and hs.is_internal = false join synonym s on s.synonym_id = hs.synonym_id join cvterm st on st.cvterm_id = s.type_id
where h.uniquename = $1 order by hs.is_current desc, st.name, s.name limit 200
