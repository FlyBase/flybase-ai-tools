-- $1 text[] of FlyBase IDs. One row per (input, status) match: current, secondary (replaced by uniquename), or obsolete.
-- This is the logic of flybase.update_ids inlined (so it also works on public chado) plus an obsolete check, batched.
with inputs as (select distinct unnest($1::text[]) as input),
cur as (
  select uniquename, name, is_obsolete from feature where uniquename = any($1::text[])
  union all select uniquename, name, is_obsolete from stock where uniquename = any($1::text[])
  union all select uniquename, name, is_obsolete from grp where uniquename = any($1::text[])
  union all select uniquename, name, is_obsolete from humanhealth where uniquename = any($1::text[])
  union all select uniquename, name, is_obsolete from cell_line where uniquename = any($1::text[])
  union all select uniquename, name, is_obsolete from library where uniquename = any($1::text[])
  union all select uniquename, name, is_obsolete from strain where uniquename = any($1::text[])
  union all select uniquename, null, is_obsolete from pub where uniquename = any($1::text[])
),
secondary as (
  select d.accession as input, f.uniquename, f.name
  from dbxref d join db on db.db_id = d.db_id and lower(db.name) = 'flybase'
  join feature_dbxref fd on fd.dbxref_id = d.dbxref_id and fd.is_current = false
  join feature f on f.feature_id = fd.feature_id and f.is_obsolete = false
  where d.accession = any($1::text[]) and left(f.uniquename, 4) = left(d.accession, 4)
)
select i.input, 'current' as status, c.uniquename, c.name from inputs i join cur c on c.uniquename = i.input and c.is_obsolete = false
union all select s.input, 'secondary', s.uniquename, s.name from secondary s
union all select i.input, 'obsolete', c.uniquename, c.name from inputs i join cur c on c.uniquename = i.input and c.is_obsolete = true
order by 1, 2, 3
