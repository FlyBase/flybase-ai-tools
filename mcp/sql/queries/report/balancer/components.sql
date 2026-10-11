-- $1 FBba. Alleles carried on the balancer and the aberrations it is a variant of.
select distinct x.relationship, x.id, x.symbol, x.type from (
  select 'carries' as relationship, a.uniquename as id, a.name as symbol, 'allele' as type
  from feature b join feature_relationship fr on fr.object_id = b.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'carried_on'
  join feature a on a.feature_id = fr.subject_id and a.is_obsolete = false where b.uniquename = $1
  union all
  select 'variant_of', ab.uniquename, ab.name, 'aberration'
  from feature b join feature_relationship fr on fr.subject_id = b.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'variant_of'
  join feature ab on ab.feature_id = fr.object_id and ab.is_obsolete = false where b.uniquename = $1
) x order by x.relationship, x.id limit 200
