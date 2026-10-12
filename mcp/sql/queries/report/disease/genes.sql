-- $1 FBhh. Human genes implicated and fly genes used as models, with the role recorded on the link.
-- Only 'gene' features: humanhealth_feature also links 'disease implicated variant' features (HGVS-named, no FBgn).
select f.uniquename as id, f.name as symbol, o.abbreviation as species,
  (select string_agg(pt.name, ', ' order by pt.name) from humanhealth_featureprop p join cvterm pt on pt.cvterm_id = p.type_id
     where p.humanhealth_feature_id = hf.humanhealth_feature_id and pt.name <> 'hh_ortholog_comment') as role,
  (select p.value from humanhealth_featureprop p join cvterm pt on pt.cvterm_id = p.type_id
     where p.humanhealth_feature_id = hf.humanhealth_feature_id and pt.name = 'hh_ortholog_comment' order by p.rank limit 1) as ortholog_comment
from humanhealth h join humanhealth_feature hf on hf.humanhealth_id = h.humanhealth_id join feature f on f.feature_id = hf.feature_id and f.is_obsolete = false
join cvterm ft on ft.cvterm_id = f.type_id and ft.name = 'gene'
join organism o on o.organism_id = f.organism_id
where h.uniquename = $1 order by (o.abbreviation = 'Hsap') desc, f.name limit 200
