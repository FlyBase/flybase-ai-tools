select f.uniquename, f.name as symbol, o.abbreviation as species,
  (select s.name from feature_synonym fs join synonym s on s.synonym_id = fs.synonym_id join cvterm st on st.cvterm_id = s.type_id
     where fs.feature_id = f.feature_id and fs.is_current and st.name = 'fullname' limit 1) as fullname,
  (select fp.value from featureprop fp join cvterm pt on pt.cvterm_id = fp.type_id where fp.feature_id = f.feature_id and pt.name = 'promoted_gene_type' limit 1) as gene_type,
  (select d.accession from feature_dbxref fd join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id
     where fd.feature_id = f.feature_id and fd.is_current and db.name = 'FlyBase Annotation IDs' limit 1) as annotation_id,
  (select fp.value from featureprop fp join cvterm pt on pt.cvterm_id = fp.type_id where fp.feature_id = f.feature_id and pt.name = 'derived_computed_cyto' limit 1) as cytology
from feature f join organism o on o.organism_id = f.organism_id
where f.uniquename = $1 and f.is_obsolete = false
