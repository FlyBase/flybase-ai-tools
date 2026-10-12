select t.uniquename, t.name, tt.name as type, t.seqlen,
  (select p.uniquename from feature_relationship pr join cvterm prt on prt.cvterm_id = pr.type_id and prt.name = 'producedby'
     join feature p on p.feature_id = pr.subject_id where pr.object_id = t.feature_id limit 1) as polypeptide
from feature g join feature_relationship fr on fr.object_id = g.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'partof'
join feature t on t.feature_id = fr.subject_id and t.is_obsolete = false
join cvterm tt on tt.cvterm_id = t.type_id and tt.name in ('mRNA','ncRNA','pre_miRNA','miRNA','tRNA','rRNA','snRNA','snoRNA','pseudogene')
where g.uniquename = $1 order by t.uniquename limit $2
