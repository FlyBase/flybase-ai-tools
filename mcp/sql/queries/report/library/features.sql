-- $1 uniquename, $2 fetch, $3 offset. Features linked to the record (clones in a collection, transgenes in a cell line).
select f.uniquename as id, f.name as symbol, ft.name as type
from library x join library_feature xf on xf.library_id = x.library_id join feature f on f.feature_id = xf.feature_id and f.is_obsolete = false join cvterm ft on ft.cvterm_id = f.type_id
where x.uniquename = $1 order by f.uniquename limit $2 offset $3
