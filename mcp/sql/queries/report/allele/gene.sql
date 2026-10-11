select g.uniquename as id, g.name as symbol from feature a join feature_relationship fr on fr.subject_id = a.feature_id
join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'alleleof' join feature g on g.feature_id = fr.object_id
where a.uniquename = $1 limit 1
