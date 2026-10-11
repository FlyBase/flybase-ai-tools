-- Both CTEs are materialized so the planner walks feature -> feature_dbxref -> dbxref by index; the open join order
-- picks a parallel seq scan of feature_dbxref (61M rows) or dbxref (42M rows) at 500 IDs.
with f as materialized (select feature_id, uniquename from feature where uniquename = any($1::text[]) and is_obsolete = false),
fx as materialized (select f.uniquename, fd.dbxref_id from f join feature_dbxref fd on fd.feature_id = f.feature_id and fd.is_current)
select fx.uniquename as id, d.accession as value, 1::bigint as n
from fx join dbxref d on d.dbxref_id = fx.dbxref_id join db on db.db_id = d.db_id and db.name = 'FlyBase Annotation IDs'
