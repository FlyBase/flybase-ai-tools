-- $1 FBtp, $2 fetch. Insertions produced from the construct.
select i.uniquename as id, i.name as symbol
from feature c join feature_relationship fr on fr.object_id = c.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'producedby'
join feature i on i.feature_id = fr.subject_id and i.is_obsolete = false and left(i.uniquename, 4) = 'FBti'
where c.uniquename = $1 order by i.uniquename limit $2
