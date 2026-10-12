-- $1 FBpp. The transcript the polypeptide is produced by.
select t.uniquename as id, t.name as symbol, tt.name as type, t.seqlen as length
from feature p join feature_relationship fr on fr.subject_id = p.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'producedby'
join feature t on t.feature_id = fr.object_id and t.is_obsolete = false and left(t.uniquename, 4) = 'FBtr' join cvterm tt on tt.cvterm_id = t.type_id
where p.uniquename = $1 order by t.uniquename limit 1
