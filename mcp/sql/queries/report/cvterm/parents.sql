-- $1 cvterm_id. Direct parents with the relationship type.
select rt.name as relationship, db.name || ':' || d.accession as term_id, p.name
from cvterm_relationship r join cvterm rt on rt.cvterm_id = r.type_id join cvterm p on p.cvterm_id = r.object_id
join dbxref d on d.dbxref_id = p.dbxref_id join db on db.db_id = d.db_id
where r.subject_id = $1 order by rt.name, p.name limit 200
