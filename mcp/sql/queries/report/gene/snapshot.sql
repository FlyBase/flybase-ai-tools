select fp.value,
  (select d.value from featureprop d join cvterm dt on dt.cvterm_id = d.type_id where d.feature_id = fp.feature_id and dt.name = 'gene_summary_date' limit 1) as date
from featureprop fp join cvterm pt on pt.cvterm_id = fp.type_id and pt.name = 'gene_summary_text'
join feature f on f.feature_id = fp.feature_id where f.uniquename = $1
