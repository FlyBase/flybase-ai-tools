-- $1 uniquename.
select distinct s.name, st.name as kind, xs.is_current
from cell_line x join cell_line_synonym xs on xs.cell_line_id = x.cell_line_id and xs.is_internal = false join synonym s on s.synonym_id = xs.synonym_id join cvterm st on st.cvterm_id = s.type_id
where x.uniquename = $1 order by xs.is_current desc, st.name, s.name limit 200
