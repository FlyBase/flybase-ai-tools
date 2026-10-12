-- $1 FBgn, $2 fetch, $3 offset. Physical interactions: the gene's interaction-specific polypeptides (-XP, associated_with the
-- gene) participate in an interaction; the partner is each other participant's current gene. An interaction whose only
-- participant feature is the gene's own polypeptide (role self, or bait and prey on one feature) is a self-interaction and
-- reports the gene as its own partner. One row per (interaction, partner gene); roles aggregate over each gene's polypeptides.
with xp as (
  select s.feature_id from feature g join feature_relationship fr on fr.object_id = g.feature_id
  join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'associated_with'
  join feature s on s.feature_id = fr.subject_id join cvterm st on st.cvterm_id = s.type_id and st.name = 'polypeptide'
  where g.uniquename = $1 and s.name like '%-XP'
),
pairs as (
  select fi.interaction_id, fo.feature_id as other_id
  from xp join feature_interaction fi on fi.feature_id = xp.feature_id
  join feature_interaction fo on fo.interaction_id = fi.interaction_id and fo.feature_id <> xp.feature_id
  union
  select fi.interaction_id, xp.feature_id
  from xp join feature_interaction fi on fi.feature_id = xp.feature_id
  where not exists (select 1 from feature_interaction f2 where f2.interaction_id = fi.interaction_id and f2.feature_id <> xp.feature_id)
),
hits as (
  select distinct pairs.interaction_id, pg.feature_id as partner_fid
  from pairs join feature_relationship pr on pr.subject_id = pairs.other_id
  join cvterm prt on prt.cvterm_id = pr.type_id and prt.name = 'associated_with'
  join feature pg on pg.feature_id = pr.object_id and left(pg.uniquename, 4) = 'FBgn' and pg.is_obsolete = false
)
select i.uniquename as interaction, it.name as type, pg.uniquename as partner_id, pg.name as partner_symbol,
  (select string_agg(distinct rr.name, ', ') from feature_interaction fo2 join cvterm rr on rr.cvterm_id = fo2.role_id
     where fo2.interaction_id = i.interaction_id and fo2.feature_id in (select feature_id from xp)) as role,
  (select string_agg(distinct rr.name, ', ') from feature_interaction fo2 join cvterm rr on rr.cvterm_id = fo2.role_id
     join feature_relationship ar on ar.subject_id = fo2.feature_id join cvterm art on art.cvterm_id = ar.type_id and art.name = 'associated_with'
     where fo2.interaction_id = i.interaction_id and ar.object_id = pg.feature_id) as partner_role,
  (select string_agg(distinct c.name, '; ') from interaction_cvterm ic join cvterm c on c.cvterm_id = ic.cvterm_id where ic.interaction_id = i.interaction_id) as detection,
  (select string_agg(distinct p.uniquename, ', ') from interaction_pub ip join pub p on p.pub_id = ip.pub_id where ip.interaction_id = i.interaction_id) as refs,
  (select string_agg(ip.value, '; ' order by ip.rank) from interactionprop ip join cvterm pt on pt.cvterm_id = ip.type_id where ip.interaction_id = i.interaction_id and pt.name = 'comment') as comment
from hits join interaction i on i.interaction_id = hits.interaction_id and i.is_obsolete = false
join cvterm it on it.cvterm_id = i.type_id join feature pg on pg.feature_id = hits.partner_fid
order by pg.name, pg.feature_id, i.uniquename, i.interaction_id limit $2 offset $3
