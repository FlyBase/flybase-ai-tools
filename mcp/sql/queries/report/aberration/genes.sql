-- $1 FBab, $2 fetch. Genes the aberration deletes, duplicates, or is a useful deficiency/duplication for.
select distinct rt.name as relationship, g.uniquename as id, g.name as symbol,
  array_position(array['deletes','molec_deletes','part_deletes','molec_partdeletes','duplicates','molec_dups','useful_Df_direct','useful_Df_from_cyto','useful_Dp_from_cyto','nondeletes','molec_nondeletes','nonduplicates'], rt.name::text) as ord
from feature ab join feature_relationship fr on fr.subject_id = ab.feature_id join cvterm rt on rt.cvterm_id = fr.type_id
  and rt.name in ('deletes', 'molec_deletes', 'part_deletes', 'molec_partdeletes', 'duplicates', 'molec_dups', 'nondeletes', 'molec_nondeletes', 'nonduplicates', 'useful_Df_direct', 'useful_Df_from_cyto', 'useful_Dp_from_cyto')
join feature g on g.feature_id = fr.object_id and g.is_obsolete = false and left(g.uniquename, 4) = 'FBgn'
where ab.uniquename = $1
order by ord, g.name
limit $2
