-- $1 FBhh. Name, category, subtype, and the OMIM phenotype accession(s).
select h.uniquename, h.name, o.abbreviation as species,
  (select p.value from humanhealthprop p join cvterm pt on pt.cvterm_id = p.type_id where p.humanhealth_id = h.humanhealth_id and pt.name = 'category' order by p.rank limit 1) as category,
  (select p.value from humanhealthprop p join cvterm pt on pt.cvterm_id = p.type_id where p.humanhealth_id = h.humanhealth_id and pt.name = 'sub_datatype' order by p.rank limit 1) as subtype,
  (select string_agg(d.accession, ', ' order by d.accession) from humanhealth_dbxref hd join dbxref d on d.dbxref_id = hd.dbxref_id join db on db.db_id = d.db_id
     where hd.humanhealth_id = h.humanhealth_id and hd.is_current and db.name = 'OMIM_PHENOTYPE') as omim_phenotype_ids
from humanhealth h join organism o on o.organism_id = h.organism_id
where h.uniquename = $1 and h.is_obsolete = false
