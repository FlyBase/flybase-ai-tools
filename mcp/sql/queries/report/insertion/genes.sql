-- $1 FBti. Genes associated with the insertion (the affected gene and any reporter gene). Genes are the subject side.
select distinct g.uniquename as id, g.name as symbol
from feature i join feature_relationship fr on fr.object_id = i.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'associated_with'
join feature g on g.feature_id = fr.subject_id and g.is_obsolete = false and left(g.uniquename, 4) = 'FBgn'
where i.uniquename = $1 order by g.name limit 50
