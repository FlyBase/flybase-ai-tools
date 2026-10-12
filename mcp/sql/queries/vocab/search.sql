-- $1 raw query, $2 query with LIKE wildcards escaped, $3 cv names to search or null for all, $4 fetch.
-- Matches term names and synonyms by substring (trigram indexes from migration 003); exact name first, then shorter names.
select t.cvterm_id, t.name, c.name as vocabulary, db.name || ':' || d.accession as term_id, t.definition,
  (select string_agg(s.name, '; ' order by s.name) from cvtermsynonym s where s.cvterm_id = t.cvterm_id) as synonyms,
  case when lower(t.name) = lower($1) then 'exact' when lower(t.name) like (lower($2) || '%') escape '\' then 'prefix' else 'substring' end as match_kind
from cvterm t join cv c on c.cv_id = t.cv_id join dbxref d on d.dbxref_id = t.dbxref_id join db on db.db_id = d.db_id
where t.is_obsolete = 0 and ($3::text[] is null or c.name = any($3::text[]))
  and (t.name ilike ('%' || $2 || '%') escape '\'
       or exists (select 1 from cvtermsynonym s where s.cvterm_id = t.cvterm_id and s.name ilike ('%' || $2 || '%') escape '\'))
order by (lower(t.name) = lower($1)) desc, (lower(t.name) like (lower($2) || '%') escape '\') desc, length(t.name), t.name
limit $4
