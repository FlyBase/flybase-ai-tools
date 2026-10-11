-- $1 FBgg, $2 fetch. Member genes with the membership kind (grpmember_feature: curated; derived_grpmember_feature: inherited from a subgroup)
-- and, for pathways, the subgroup label from gene_group.pathway_member.
select distinct f.uniquename as id, f.name as symbol, mt.name as membership, pm.group_member_label as label
from grp g join grpmember gm on gm.grp_id = g.grp_id join cvterm mt on mt.cvterm_id = gm.type_id
join feature_grpmember fg on fg.grpmember_id = gm.grpmember_id join feature f on f.feature_id = fg.feature_id and f.is_obsolete = false
left join gene_group.pathway_member pm on pm.fbgg_id = g.uniquename and pm.fbgn_id = f.uniquename
where g.uniquename = $1 order by mt.name, f.name limit $2
