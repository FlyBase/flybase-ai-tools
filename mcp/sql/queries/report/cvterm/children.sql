-- $1 cvterm_id, $2 fetch, $3 offset. Direct children with the relationship type.
select rt.name as relationship, db.name || ':' || d.accession as term_id, c.name
from cvterm_relationship r join cvterm rt on rt.cvterm_id = r.type_id join cvterm c on c.cvterm_id = r.subject_id and c.is_obsolete = 0
join dbxref d on d.dbxref_id = c.dbxref_id join db on db.db_id = d.db_id
where r.object_id = $1 order by rt.name, c.name, term_id limit $2 offset $3
