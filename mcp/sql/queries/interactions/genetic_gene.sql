-- $1 FBgn, $2 fetch, $3 offset. Genetic interactions recorded as interacts_genetically relationships in either direction,
-- with the allele named on the relationship (if any) and the supporting references.
select x.partner_id, x.partner_symbol, x.direction, x.allele, x.refs from (
  select t.uniquename as partner_id, t.name as partner_symbol, 'subject' as direction,
    (select string_agg(distinct p.value, '; ') from feature_relationshipprop p where p.feature_relationship_id = fr.feature_relationship_id) as allele,
    (select string_agg(distinct pb.uniquename, ', ') from feature_relationship_pub rp join pub pb on pb.pub_id = rp.pub_id where rp.feature_relationship_id = fr.feature_relationship_id) as refs
  from feature g join feature_relationship fr on fr.subject_id = g.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'interacts_genetically'
  join feature t on t.feature_id = fr.object_id and t.is_obsolete = false where g.uniquename = $1
  union all
  select s.uniquename, s.name, 'object',
    (select string_agg(distinct p.value, '; ') from feature_relationshipprop p where p.feature_relationship_id = fr.feature_relationship_id),
    (select string_agg(distinct pb.uniquename, ', ') from feature_relationship_pub rp join pub pb on pb.pub_id = rp.pub_id where rp.feature_relationship_id = fr.feature_relationship_id)
  from feature g join feature_relationship fr on fr.object_id = g.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'interacts_genetically'
  join feature s on s.feature_id = fr.subject_id and s.is_obsolete = false where g.uniquename = $1
) x order by x.partner_symbol, x.direction, x.allele limit $2 offset $3
