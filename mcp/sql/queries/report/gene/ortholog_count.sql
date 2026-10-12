select count(distinct fr.object_id) as n from feature f join feature_relationship fr on fr.subject_id = f.feature_id
join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'orthologous_to' where f.uniquename = $1
