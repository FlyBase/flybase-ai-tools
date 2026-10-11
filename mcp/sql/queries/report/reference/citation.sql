select p.uniquename, p.title, p.pyear as year, p.miniref, p.volume, p.issue, p.pages, pt.name as type,
  (select d.accession from pub_dbxref pd join dbxref d on d.dbxref_id = pd.dbxref_id join db on db.db_id = d.db_id where pd.pub_id = p.pub_id and lower(db.name) = 'pubmed' limit 1) as pubmed_id,
  (select d.accession from pub_dbxref pd join dbxref d on d.dbxref_id = pd.dbxref_id join db on db.db_id = d.db_id where pd.pub_id = p.pub_id and lower(db.name) = 'doi' limit 1) as doi
from pub p join cvterm pt on pt.cvterm_id = p.type_id where p.uniquename = $1 and p.is_obsolete = false
