-- $1 FBtr. The polypeptide produced by the transcript, if any.
select p.uniquename as id, p.name as symbol, p.seqlen as length
from feature t join feature_relationship fr on fr.object_id = t.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'producedby'
join feature p on p.feature_id = fr.subject_id and p.is_obsolete = false and left(p.uniquename, 4) = 'FBpp'
where t.uniquename = $1 order by p.uniquename limit 1
