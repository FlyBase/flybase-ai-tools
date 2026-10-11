-- $1 FBtr or FBpp. The gene: directly (transcript partof gene) or through the transcript (polypeptide producedby transcript partof gene).
select g.uniquename as id, g.name as symbol
from feature x
left join feature_relationship pb on pb.subject_id = x.feature_id and pb.type_id = (select cvterm_id from cvterm where name = 'producedby' and cv_id = (select cv_id from cv where name = 'relationship type'))
join feature_relationship fr on fr.subject_id = coalesce(pb.object_id, x.feature_id)
join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'partof'
join feature g on g.feature_id = fr.object_id and g.is_obsolete = false and left(g.uniquename, 4) = 'FBgn'
where x.uniquename = $1 order by g.uniquename limit 1
