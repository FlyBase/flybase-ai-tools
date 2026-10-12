-- $1 FBgg. GO and other terms attached to the group.
select distinct cv.name as vocabulary, c.name as term, db.name || ':' || d.accession as term_id
from grp g join grp_cvterm gc on gc.grp_id = g.grp_id and gc.is_not = false join cvterm c on c.cvterm_id = gc.cvterm_id join cv on cv.cv_id = c.cv_id
join dbxref d on d.dbxref_id = c.dbxref_id join db on db.db_id = d.db_id
where g.uniquename = $1 order by cv.name, c.name limit 100
