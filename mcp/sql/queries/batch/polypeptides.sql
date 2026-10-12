select g.uniquename as id, array_to_string((array_agg(p.uniquename order by p.uniquename))[1:50], '|') as value, count(*) as n
from feature g join feature_relationship fr on fr.object_id = g.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'partof'
join feature t on t.feature_id = fr.subject_id and t.is_obsolete = false and left(t.uniquename, 4) = 'FBtr'
join feature_relationship pr on pr.object_id = t.feature_id join cvterm prt on prt.cvterm_id = pr.type_id and prt.name = 'producedby'
join feature p on p.feature_id = pr.subject_id and p.is_obsolete = false and left(p.uniquename, 4) = 'FBpp'
where g.uniquename = any($1::text[]) group by g.uniquename
