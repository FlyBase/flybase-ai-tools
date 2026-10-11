with cur as (
  select uniquename, name, is_obsolete from feature where uniquename = $1
  union all select uniquename, name, is_obsolete from stock where uniquename = $1
  union all select uniquename, name, is_obsolete from grp where uniquename = $1
  union all select uniquename, name, is_obsolete from humanhealth where uniquename = $1
  union all select uniquename, null, is_obsolete from pub where uniquename = $1
),
secondary as (
  select f.uniquename, f.name
  from feature f join feature_dbxref fd on fd.feature_id = f.feature_id
  join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id
  where fd.is_current = false and d.accession = $1 and lower(db.name) = 'flybase'
    and left(f.uniquename, 4) = left($1, 4) and f.is_obsolete = false
)
select 'current' as status, uniquename, name from cur where is_obsolete = false
union all select 'secondary', uniquename, name from secondary
union all select 'obsolete', uniquename, name from cur where is_obsolete = true
