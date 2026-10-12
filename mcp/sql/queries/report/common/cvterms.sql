-- $1 uniquename. Distinct controlled-vocabulary terms attached through feature_cvterm.
select distinct c.name as vocabulary, t.name as term, db.name || ':' || d.accession as term_id
from feature f join feature_cvterm fc on fc.feature_id = f.feature_id and fc.is_not = false
join cvterm t on t.cvterm_id = fc.cvterm_id join cv c on c.cv_id = t.cv_id join dbxref d on d.dbxref_id = t.dbxref_id join db on db.db_id = d.db_id
where f.uniquename = $1 order by c.name, t.name limit 200
