-- $1 raw query, $2 fetch count, $3 offset, $4 query with LIKE wildcards escaped by backslash.
select t.name, c.name as cv, d.accession, db.name as db, t.definition,
       case when lower(t.name) = lower($1) then 'exact' else 'substring' end as match_kind
from cvterm t join cv c on c.cv_id = t.cv_id join dbxref d on d.dbxref_id = t.dbxref_id join db on db.db_id = d.db_id
where t.is_obsolete = 0
  and (t.name ilike ('%' || $4 || '%') escape '\'
       or exists (select 1 from cvtermsynonym s where s.cvterm_id = t.cvterm_id and s.name ilike ('%' || $4 || '%') escape '\'))
order by (lower(t.name) = lower($1)) desc, length(t.name), t.name
limit $2 offset $3
