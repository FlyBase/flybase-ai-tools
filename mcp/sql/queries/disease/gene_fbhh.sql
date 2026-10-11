-- $1 FBgn or FBal. Human disease model entries (FBhh) that cite the feature, with the role recorded on the link.
select h.uniquename as id, h.name,
  (select string_agg(pt.name, ', ' order by pt.name) from humanhealth_featureprop p join cvterm pt on pt.cvterm_id = p.type_id
     where p.humanhealth_feature_id = hf.humanhealth_feature_id and pt.name <> 'hh_ortholog_comment') as role
from feature f join humanhealth_feature hf on hf.feature_id = f.feature_id join humanhealth h on h.humanhealth_id = hf.humanhealth_id and h.is_obsolete = false
where f.uniquename = $1 order by h.name limit 200
