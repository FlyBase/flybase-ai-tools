-- $1 db name (GO, FBbt, FBcv, FBdv, DOID, SO, MI), $2 accession. The term row by its public ID.
select t.cvterm_id, t.name, c.name as vocabulary, t.definition, t.is_obsolete <> 0 as is_obsolete, db.name || ':' || d.accession as term_id,
  (select string_agg(p.value, '; ' order by p.rank) from cvtermprop p join cvterm pt on pt.cvterm_id = p.type_id where p.cvterm_id = t.cvterm_id and pt.name = 'comment') as comment
from cvterm t join cv c on c.cv_id = t.cv_id join dbxref d on d.dbxref_id = t.dbxref_id join db on db.db_id = d.db_id
where db.name = $1 and d.accession = $2
