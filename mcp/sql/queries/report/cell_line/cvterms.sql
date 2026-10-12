-- $1 uniquename. Controlled-vocabulary terms attached to the record.
select distinct cv.name as vocabulary, c.name as term, db.name || ':' || d.accession as term_id
from cell_line x join cell_line_cvterm xc on xc.cell_line_id = x.cell_line_id join cvterm c on c.cvterm_id = xc.cvterm_id join cv on cv.cv_id = c.cv_id
join dbxref d on d.dbxref_id = c.dbxref_id join db on db.db_id = d.db_id
where x.uniquename = $1 order by cv.name, c.name limit 200
