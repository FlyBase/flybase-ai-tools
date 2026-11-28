-- $1 FBto, $2 fetch. Only terms curated as tool uses.
select db.name || ':' || dx.accession as id, t.name,
  coalesce(array_agg(distinct p.uniquename order by p.uniquename)
    filter (where p.uniquename ~ '^FBrf[0-9]+$'), '{}') as reference_ids
from feature f join feature_cvterm fc using (feature_id)
join cvterm t using (cvterm_id)
join dbxref dx on dx.dbxref_id = t.dbxref_id join db using (db_id)
join pub p using (pub_id)
where f.uniquename = $1 and not fc.is_not
  and exists (
    select from feature_cvtermprop cp join cvterm pt on pt.cvterm_id = cp.type_id
    where cp.feature_cvterm_id = fc.feature_cvterm_id and pt.name = 'tool_uses'
  )
group by 1, 2 order by t.name, id limit $2
