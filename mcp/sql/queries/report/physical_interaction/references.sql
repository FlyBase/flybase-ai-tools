-- $1 FBig, $2 fetch, $3 offset. Deduplicate the papers linked to the current experiment records.
with papers as (
  select distinct ip.pub_id
  from interaction_group g
  join interaction_group_feature_interaction j using (interaction_group_id)
  join feature_interaction fi using (feature_interaction_id)
  join interaction i using (interaction_id)
  join interaction_pub ip using (interaction_id)
  where g.uniquename = $1 and not g.is_obsolete and not i.is_obsolete
)
select p.uniquename, p.miniref, p.pyear,
  (select d.accession from pub_dbxref pd join dbxref d using (dbxref_id) join db using (db_id)
   where pd.pub_id = p.pub_id and lower(db.name) = 'pubmed' order by d.accession limit 1) as pmid
from papers join pub p using (pub_id)
where not p.is_obsolete
order by p.pyear desc nulls last, p.uniquename
limit $2 offset $3
