select distinct on (f.uniquename) f.uniquename as id, s.name as value, 1::bigint as n
from feature f join feature_synonym fs on fs.feature_id = f.feature_id and fs.is_current and fs.is_internal = false
join synonym s on s.synonym_id = fs.synonym_id join cvterm st on st.cvterm_id = s.type_id and st.name = 'fullname'
where f.uniquename = any($1::text[]) and f.is_obsolete = false order by f.uniquename, s.synonym_id
