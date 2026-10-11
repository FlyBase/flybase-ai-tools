select f.uniquename as id, d.accession as value, 1::bigint as n
from feature f join feature_dbxref fd on fd.feature_id = f.feature_id and fd.is_current join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id and db.name = 'FlyBase Annotation IDs'
where f.uniquename = any($1::text[]) and f.is_obsolete = false
