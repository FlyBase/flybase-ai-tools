select a.uniquename as id, g.uniquename || ' ' || g.name as value, 1::bigint as n
from feature a join feature_relationship fr on fr.subject_id = a.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'alleleof'
join feature g on g.feature_id = fr.object_id and g.is_obsolete = false
where a.uniquename = any($1::text[]) and a.is_obsolete = false
