-- $1 uniquename of any feature. One row: symbol, full name, species, SO type, and stored sequence length.
select f.uniquename, f.name as symbol, o.abbreviation as species, t.name as type, f.seqlen,
  (select s.name from feature_synonym fs join synonym s on s.synonym_id = fs.synonym_id join cvterm st on st.cvterm_id = s.type_id
     where fs.feature_id = f.feature_id and fs.is_current and fs.is_internal = false and st.name = 'fullname' order by s.synonym_id limit 1) as fullname
from feature f join organism o on o.organism_id = f.organism_id join cvterm t on t.cvterm_id = f.type_id
where f.uniquename = $1 and f.is_obsolete = false
