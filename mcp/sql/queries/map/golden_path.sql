-- $1 species abbreviation, $2 chromosome arm. One row when the arm is a golden_path feature of the species.
select src.uniquename
from feature src join organism o on o.organism_id = src.organism_id and o.abbreviation = $1
join cvterm st on st.cvterm_id = src.type_id and st.name = 'golden_path'
where src.uniquename = $2 and src.is_obsolete = false
limit 1
