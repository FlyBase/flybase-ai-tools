-- $1 FBto, $2 fetch. Historical records can have allele IDs from before the
-- experimental tool data class was introduced.
select distinct d.accession as id
from feature f join feature_dbxref fd using (feature_id)
join dbxref d on d.dbxref_id = fd.dbxref_id join db using (db_id)
where f.uniquename = $1 and not fd.is_current and db.name = 'FlyBase'
  and d.accession <> f.uniquename
order by d.accession limit $2
