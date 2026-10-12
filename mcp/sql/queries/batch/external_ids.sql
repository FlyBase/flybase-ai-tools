-- Both CTEs are materialized so the planner walks feature -> feature_dbxref -> dbxref by index; the open join order
-- picks a parallel seq scan of feature_dbxref (61M rows) or dbxref (42M rows) at 500 IDs.
with f as materialized (select feature_id, uniquename from feature where uniquename = any($1::text[]) and is_obsolete = false),
fx as materialized (select f.uniquename, fd.dbxref_id from f join feature_dbxref fd on fd.feature_id = f.feature_id and fd.is_current)
select fx.uniquename as id, array_to_string((array_agg(distinct db.name || ':' || d.accession order by db.name || ':' || d.accession))[1:50], '|') as value, count(distinct db.name || ':' || d.accession) as n
from fx join dbxref d on d.dbxref_id = fx.dbxref_id join db on db.db_id = d.db_id
where db.name not in ('FlyBase', 'FlyBase Annotation IDs') group by fx.uniquename
