-- $1 uniquename. Name, species, and (libraries) the type.
select x.uniquename, x.name, o.abbreviation as species, t.name as type
from library x join organism o on o.organism_id = x.organism_id join cvterm t on t.cvterm_id = x.type_id
where x.uniquename = $1 and x.is_obsolete = false
