-- $1 FBgn, $2 fetch, $3 offset. Physical interactions: the gene's interaction-specific polypeptide (-XP, associated_with the gene)
-- participates in an interaction; the partner is the other participant's gene.
with xp as (
  select s.feature_id from feature g join feature_relationship fr on fr.object_id = g.feature_id
  join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'associated_with'
  join feature s on s.feature_id = fr.subject_id join cvterm st on st.cvterm_id = s.type_id and st.name = 'polypeptide'
  where g.uniquename = $1 and s.name like '%-XP'
)
select i.uniquename as interaction, it.name as type, pg.uniquename as partner_id, pg.name as partner_symbol,
  (select string_agg(distinct rr.name, ', ') from feature_interaction fo2 join cvterm rr on rr.cvterm_id = fo2.role_id where fo2.interaction_id = i.interaction_id and fo2.feature_id = xp.feature_id) as role,
  (select string_agg(distinct rr.name, ', ') from feature_interaction fo2 join cvterm rr on rr.cvterm_id = fo2.role_id where fo2.interaction_id = i.interaction_id and fo2.feature_id = fo.feature_id) as partner_role,
  (select string_agg(distinct c.name, '; ') from interaction_cvterm ic join cvterm c on c.cvterm_id = ic.cvterm_id where ic.interaction_id = i.interaction_id) as detection,
  (select string_agg(distinct p.uniquename, ', ') from interaction_pub ip join pub p on p.pub_id = ip.pub_id where ip.interaction_id = i.interaction_id) as refs,
  (select string_agg(ip.value, '; ' order by ip.rank) from interactionprop ip join cvterm pt on pt.cvterm_id = ip.type_id where ip.interaction_id = i.interaction_id and pt.name = 'comment') as comment
from xp join feature_interaction fi on fi.feature_id = xp.feature_id
join interaction i on i.interaction_id = fi.interaction_id and i.is_obsolete = false join cvterm it on it.cvterm_id = i.type_id
join feature_interaction fo on fo.interaction_id = i.interaction_id and fo.feature_id <> xp.feature_id
join feature_relationship pr on pr.subject_id = fo.feature_id join cvterm prt on prt.cvterm_id = pr.type_id and prt.name = 'associated_with'
join feature pg on pg.feature_id = pr.object_id and left(pg.uniquename, 4) = 'FBgn'
order by pg.name, i.uniquename limit $2 offset $3
