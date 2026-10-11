-- $1 query with LIKE wildcards escaped by backslash, $2 escaped author surname or null, $3 year text or null,
-- $4 fetch count, $5 offset.
select p.uniquename, p.title, p.pyear, p.miniref,
       (select string_agg(a.surname, ', ' order by a.rank) from pubauthor a where a.pub_id = p.pub_id) as authors
from pub p
where p.is_obsolete = false
  and (p.title ilike ('%' || $1 || '%') escape '\' or p.miniref ilike ('%' || $1 || '%') escape '\')
  and ($2::text is null or exists (select 1 from pubauthor a where a.pub_id = p.pub_id and a.surname ilike $2 escape '\'))
  and ($3::text is null or p.pyear = $3)
order by p.pyear desc nulls last, p.uniquename
limit $4 offset $5
