select f.uniquename, f.name, o.abbreviation as species, src.uniquename as chromosome, fl.fmin, fl.fmax, fl.strand
from feature f join organism o on o.organism_id = f.organism_id
join featureloc fl on fl.feature_id = f.feature_id and fl.rank = 0 and fl.locgroup = 0
join feature src on src.feature_id = fl.srcfeature_id
join cvterm st on st.cvterm_id = src.type_id and st.name = 'golden_path'
where f.uniquename = $1 and f.is_obsolete = false
