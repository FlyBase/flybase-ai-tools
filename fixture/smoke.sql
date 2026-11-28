select 'notch' as check, count(*) = 1 as ok from feature where uniquename = 'FBgn0004647'
union all select 'notch transcripts', count(*) >= 2 from feature_relationship fr join feature t on t.feature_id = fr.subject_id
  where fr.object_id = (select feature_id from feature where uniquename = 'FBgn0004647')
union all select 'golden path X complete', exists (
  select 1 from feature where uniquename = 'X' and organism_id = (select organism_id from organism where abbreviation = 'Dmel')
  and type_id = (select cvterm_id from cvterm where name = 'golden_path' and cv_id = (select cv_id from cv where name = 'SO'))
  and seqlen > 0 and length(residues) = seqlen)
union all select 'update_ids executable', has_function_privilege('flybase.update_ids(text)', 'execute')
union all select 'phenstatements for N[55e11]', count(*) > 100 from phenstatement where genotype_id in (select genotype_id from feature_genotype where feature_id = (select feature_id from feature where uniquename = 'FBal0012701'))
union all select 'stock', count(*) = 1 from stock where uniquename = 'FBst0028813'
union all select 'physical interaction', count(distinct fi.interaction_id) = 4
  from interaction_group g join interaction_group_feature_interaction j using (interaction_group_id)
  join feature_interaction fi using (feature_interaction_id) where g.uniquename = 'FBig0000104993'
union all select 'pub', count(*) > 100000 from pub;
