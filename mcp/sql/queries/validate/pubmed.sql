-- $1 text[] of PubMed IDs (digits). References carrying them; db names vary in case.
select x.input, p.uniquename, p.miniref
from unnest($1::text[]) x(input)
join dbxref d on d.accession = x.input join db on db.db_id = d.db_id and lower(db.name) = 'pubmed'
join pub_dbxref pd on pd.dbxref_id = d.dbxref_id join pub p on p.pub_id = pd.pub_id and p.is_obsolete = false
order by x.input, p.uniquename limit 5000
