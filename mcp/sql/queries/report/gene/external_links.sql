select db.name as db, d.accession, case when db.urlprefix is not null then db.urlprefix || d.accession end as url
from feature f join feature_dbxref fd on fd.feature_id = f.feature_id and fd.is_current
join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id
where f.uniquename = $1 and db.name not in ('FlyBase', 'FlyBase Annotation IDs') order by db.name, d.accession limit 200
