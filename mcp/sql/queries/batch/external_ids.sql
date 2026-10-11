select f.uniquename as id, array_to_string((array_agg(distinct db.name || ':' || d.accession order by db.name || ':' || d.accession))[1:50], '|') as value, count(distinct db.name || ':' || d.accession) as n
from feature f join feature_dbxref fd on fd.feature_id = f.feature_id and fd.is_current join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id
where f.uniquename = any($1::text[]) and f.is_obsolete = false and db.name not in ('FlyBase', 'FlyBase Annotation IDs') group by f.uniquename
