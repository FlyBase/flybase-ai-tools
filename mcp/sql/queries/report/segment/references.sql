-- The recombinant construct bibliography includes the evidence for annotations and selected edges.
with segment as (select feature_id from feature where uniquename = $1 and not is_obsolete),
edges as (
  select r.feature_relationship_id from segment f join feature_relationship r on r.object_id = f.feature_id
  join cvterm t on t.cvterm_id = r.type_id join feature s on s.feature_id = r.subject_id and not s.is_obsolete
  where t.name in ('partof','producedby','in_vitro_descendant_of')
     or (t.name in ('associated_with','derived_tp_assoc_alleles') and s.uniquename ~ '^FBal[0-9]+$')
  union all
  select r.feature_relationship_id from segment f join feature_relationship r on r.subject_id = f.feature_id
  join cvterm t on t.cvterm_id = r.type_id join feature s on s.feature_id = r.object_id and not s.is_obsolete
  where t.name in ('in_vitro_descendant_of','has_reg_region','encodes_tool','tagged_with','carries_tool','marked_with')
), citations as (
  select pub_id from segment join feature_pub using (feature_id)
  union select pub_id from segment join feature_synonym using (feature_id)
  union select pp.pub_id from segment join featureprop using (feature_id) join featureprop_pub pp using (featureprop_id)
  union select pub_id from edges join feature_relationship_pub using (feature_relationship_id)
)
select p.uniquename,p.miniref,p.pyear,p.is_obsolete,
  (select min(d.accession) from pub_dbxref pd join dbxref d using (dbxref_id) join db using (db_id)
   where pd.pub_id=p.pub_id and lower(db.name)='pubmed') as pmid
from citations join pub p using (pub_id)
where p.uniquename ~ '^FBrf[0-9]+$'
order by p.pyear desc nulls last,p.uniquename limit $2 offset $3
