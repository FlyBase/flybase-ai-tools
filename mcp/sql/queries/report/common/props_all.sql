-- $1 uniquename. Every public featureprop except curation bookkeeping, for the generic feature report.
select pt.name as type, fp.value, fp.rank
from feature f join featureprop fp on fp.feature_id = f.feature_id join cvterm pt on pt.cvterm_id = fp.type_id
where f.uniquename = $1 and fp.value is not null
  and pt.name not in ('internalnotes', 'owner', 'data_origin', 'date', 'category_of_feature_name') and pt.name not like 'derived_stock_%'
order by pt.name, fp.rank limit 200
