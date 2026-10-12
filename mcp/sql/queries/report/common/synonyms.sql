select distinct s.name, st.name as kind, fs.is_current
from feature f join feature_synonym fs on fs.feature_id = f.feature_id and fs.is_internal = false
join synonym s on s.synonym_id = fs.synonym_id join cvterm st on st.cvterm_id = s.type_id
where f.uniquename = $1 order by fs.is_current desc, st.name, s.name limit 200
