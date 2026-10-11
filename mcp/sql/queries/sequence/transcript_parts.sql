select p.uniquename, pt.name as type, rt.name as rel, fl.fmin, fl.fmax, fl.strand, p.residues
from feature t join feature_relationship fr on fr.object_id = t.feature_id
join cvterm rt on rt.cvterm_id = fr.type_id and rt.name in ('partof','producedby')
join feature p on p.feature_id = fr.subject_id and p.is_obsolete = false
join cvterm pt on pt.cvterm_id = p.type_id and pt.name in ('exon','CDS','five_prime_UTR','three_prime_UTR','intron','polypeptide')
left join featureloc fl on fl.feature_id = p.feature_id and fl.rank = 0 and fl.locgroup = 0
where t.uniquename = $1
order by fl.fmin
