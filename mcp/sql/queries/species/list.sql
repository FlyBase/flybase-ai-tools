select o.abbreviation, o.genus, o.species, o.common_name,
       exists (select 1 from feature f join cvterm t on t.cvterm_id = f.type_id
               where f.organism_id = o.organism_id and t.name = 'golden_path' and f.is_obsolete = false) as has_assembly
from organism o
where o.abbreviation is not null
order by (o.abbreviation = 'Dmel') desc, has_assembly desc, o.genus, o.species
limit 1000
