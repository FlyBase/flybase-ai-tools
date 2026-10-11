select c.name as aspect, d.accession, t.name as term,
  (select string_agg(p.value, '; ') from feature_cvtermprop p join cvterm pt on pt.cvterm_id = p.type_id where p.feature_cvterm_id = fc.feature_cvterm_id and pt.name = 'evidence_code') as evidence,
  (select string_agg(pt.name, '; ') from feature_cvtermprop p join cvterm pt on pt.cvterm_id = p.type_id where p.feature_cvterm_id = fc.feature_cvterm_id
     and pt.name in ('enables','involved_in','located_in','part_of','contributes_to','colocalizes_with','acts_upstream_of','acts_upstream_of_positive_effect','acts_upstream_of_negative_effect','is_active_in')) as qualifier,
  exists (select 1 from feature_cvtermprop p join cvterm pt on pt.cvterm_id = p.type_id where p.feature_cvterm_id = fc.feature_cvterm_id and pt.name = 'evidence_code'
     and p.value ~* '^inferred from (mutant phenotype|direct assay|physical interaction|genetic interaction|expression pattern|experiment|high throughput)') as is_experimental
from feature f join feature_cvterm fc on fc.feature_id = f.feature_id and fc.is_not = false
join cvterm t on t.cvterm_id = fc.cvterm_id join cv c on c.cv_id = t.cv_id and c.name in ('biological_process','molecular_function','cellular_component')
join dbxref d on d.dbxref_id = t.dbxref_id
where f.uniquename = $1
order by c.name, is_experimental desc, t.name
