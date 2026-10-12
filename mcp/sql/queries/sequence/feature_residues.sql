select f.uniquename, f.name, t.name as type, f.seqlen, f.residues, o.abbreviation as species
from feature f join cvterm t on t.cvterm_id = f.type_id join organism o on o.organism_id = f.organism_id
where f.uniquename = $1 and f.is_obsolete = false
