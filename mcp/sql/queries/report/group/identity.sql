-- $1 FBgg. Symbol, full name, description, and whether the group is a signaling pathway.
select g.uniquename, g.name as symbol, t.name as type,
  (select s.name from grp_synonym gs join synonym s on s.synonym_id = gs.synonym_id join cvterm st on st.cvterm_id = s.type_id
     where gs.grp_id = g.grp_id and gs.is_current and gs.is_internal = false and st.name = 'fullname' order by s.synonym_id limit 1) as fullname,
  (select p.value from grpprop p join cvterm pt on pt.cvterm_id = p.type_id where p.grp_id = g.grp_id and pt.name = 'gg_description' order by p.rank limit 1) as description,
  (select p.value from grpprop p join cvterm pt on pt.cvterm_id = p.type_id where p.grp_id = g.grp_id and pt.name = 'gg_review_date' order by p.rank limit 1) as review_date,
  gene_group.is_pathway(g.uniquename) as is_pathway,
  (select string_agg(c.name, '; ' order by c.name) from grp_cvterm gc join cvterm c on c.cvterm_id = gc.cvterm_id join cv on cv.cv_id = c.cv_id
     where gc.grp_id = g.grp_id and cv.name = 'FlyBase miscellaneous CV') as category
from grp g join cvterm t on t.cvterm_id = g.type_id
where g.uniquename = $1 and g.is_obsolete = false
