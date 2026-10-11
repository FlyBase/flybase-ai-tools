select f.uniquename, f.seqlen, substr(f.residues, $3 + 1, $4) as seq
from feature f join cvterm t on t.cvterm_id = f.type_id and t.name = 'golden_path'
join organism o on o.organism_id = f.organism_id and o.abbreviation = $1
where f.uniquename = $2 and f.is_obsolete = false
