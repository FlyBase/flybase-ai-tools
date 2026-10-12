select f.uniquename as id, array_to_string((array_agg(distinct d.accession order by d.accession))[1:50], '|') as value, count(distinct d.accession) as n
from feature f join feature_pub fp on fp.feature_id = f.feature_id join pub p on p.pub_id = fp.pub_id and p.is_obsolete = false
join pub_dbxref pd on pd.pub_id = p.pub_id join dbxref d on d.dbxref_id = pd.dbxref_id join db on db.db_id = d.db_id and lower(db.name) = 'pubmed'
where f.uniquename = any($1::text[]) and f.is_obsolete = false group by f.uniquename
