-- $1 FBti, $2 fetch. Alleles associated with the insertion (alleles are the subject side).
select distinct a.uniquename as id, a.name as symbol
from feature i join feature_relationship fr on fr.object_id = i.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'associated_with'
join feature a on a.feature_id = fr.subject_id and a.is_obsolete = false and left(a.uniquename, 4) = 'FBal'
where i.uniquename = $1 order by a.uniquename limit $2
