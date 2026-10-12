-- $1 uniquename. Name, species, and (libraries) the type.
select x.uniquename, x.name, o.abbreviation as species, null::text as type
from strain x join organism o on o.organism_id = x.organism_id
where x.uniquename = $1 and x.is_obsolete = false
