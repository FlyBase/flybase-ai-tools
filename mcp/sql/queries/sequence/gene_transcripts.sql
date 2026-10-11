select t.uniquename, t.name, tt.name as type, t.seqlen, t.residues, fl.fmin, fl.fmax, fl.strand
from feature g join feature_relationship fr on fr.object_id = g.feature_id
join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'partof'
join feature t on t.feature_id = fr.subject_id and t.is_obsolete = false
join cvterm tt on tt.cvterm_id = t.type_id and tt.name in ('mRNA','ncRNA','pre_miRNA','miRNA','tRNA','rRNA','snRNA','snoRNA','pseudogene')
left join featureloc fl on fl.feature_id = t.feature_id and fl.rank = 0 and fl.locgroup = 0
where g.uniquename = $1
order by t.uniquename
