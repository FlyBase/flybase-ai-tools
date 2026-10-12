select g.uniquename as id, array_to_string((array_agg(t.uniquename order by t.uniquename))[1:50], '|') as value, count(*) as n
from feature g join feature_relationship fr on fr.object_id = g.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'partof'
join feature t on t.feature_id = fr.subject_id and t.is_obsolete = false and left(t.uniquename, 4) = 'FBtr'
where g.uniquename = any($1::text[]) group by g.uniquename
