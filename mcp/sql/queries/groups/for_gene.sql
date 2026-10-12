-- $1 FBgn. Gene groups and pathways the gene belongs to, with the membership kind.
select distinct g.uniquename as id, g.name as symbol,
  (select s.name from grp_synonym gs join synonym s on s.synonym_id = gs.synonym_id join cvterm st on st.cvterm_id = s.type_id where gs.grp_id = g.grp_id and gs.is_current and st.name = 'fullname' limit 1) as name,
  gene_group.is_pathway(g.uniquename) as is_pathway, mt.name as membership
from feature f join feature_grpmember fg on fg.feature_id = f.feature_id join grpmember gm on gm.grpmember_id = fg.grpmember_id
join cvterm mt on mt.cvterm_id = gm.type_id join grp g on g.grp_id = gm.grp_id and g.is_obsolete = false
where f.uniquename = $1 order by is_pathway desc, g.name limit 200
