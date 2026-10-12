select f.uniquename, f.name as symbol, o.abbreviation as species, a.is_construct, a.known_lesion, a.stocks_count, a.paper_count,
  (select s.name from feature_synonym fs join synonym s on s.synonym_id = fs.synonym_id join cvterm st on st.cvterm_id = s.type_id
     where fs.feature_id = f.feature_id and fs.is_current and st.name = 'fullname' limit 1) as fullname
from feature f join organism o on o.organism_id = f.organism_id left join gene.allele a on a.fbal_id = f.uniquename
where f.uniquename = $1 and f.is_obsolete = false
