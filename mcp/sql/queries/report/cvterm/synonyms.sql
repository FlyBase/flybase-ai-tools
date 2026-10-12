-- $1 cvterm_id.
select s.name, st.name as scope from cvtermsynonym s left join cvterm st on st.cvterm_id = s.type_id
where s.cvterm_id = $1 order by s.name limit 200
