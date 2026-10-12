select f.uniquename as id, array_to_string((array_agg(distinct s.name order by s.name))[1:50], '|') as value, count(distinct s.name) as n
from feature f join feature_synonym fs on fs.feature_id = f.feature_id and fs.is_internal = false join synonym s on s.synonym_id = fs.synonym_id
where f.uniquename = any($1::text[]) and f.is_obsolete = false group by f.uniquename
