select f.uniquename as id, split_part(fp.value, ';', 1) as value, 1::bigint as n
from feature f join featureprop fp on fp.feature_id = f.feature_id join cvterm pt on pt.cvterm_id = fp.type_id and pt.name = 'derived_computed_cyto'
where f.uniquename = any($1::text[]) and f.is_obsolete = false
