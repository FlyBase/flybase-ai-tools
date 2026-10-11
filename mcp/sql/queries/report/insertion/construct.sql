-- $1 FBti. The transgenic construct that produced the insertion.
select c.uniquename as id, c.name as symbol
from feature i join feature_relationship fr on fr.subject_id = i.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'producedby'
join feature c on c.feature_id = fr.object_id and c.is_obsolete = false and left(c.uniquename, 4) = 'FBtp'
where i.uniquename = $1 order by c.uniquename limit 1
