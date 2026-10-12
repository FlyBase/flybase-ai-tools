-- $1 uniquename, $2 featureprop type names to fetch. Rows ordered by type then rank.
select pt.name as type, fp.value, fp.rank
from feature f join featureprop fp on fp.feature_id = f.feature_id join cvterm pt on pt.cvterm_id = fp.type_id
where f.uniquename = $1 and pt.name = any($2::text[]) and fp.value is not null
order by pt.name, fp.rank limit 200
