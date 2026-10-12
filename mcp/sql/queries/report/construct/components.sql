-- $1 FBtp. What the construct was built from and carries: plasmids, progenitor constructs, markers, tools, regulatory regions.
select rt.name as relationship, t.uniquename as id, t.name as symbol, tt.name as type
from feature c join feature_relationship fr on fr.subject_id = c.feature_id join cvterm rt on rt.cvterm_id = fr.type_id
  and rt.name in ('in_vitro_descendant_of', 'descendant_of', 'marked_with', 'belongs_to', 'carries_tool', 'encodes_tool', 'tagged_with', 'has_reg_region', 'partof')
join feature t on t.feature_id = fr.object_id and t.is_obsolete = false join cvterm tt on tt.cvterm_id = t.type_id
where c.uniquename = $1 order by rt.name, t.uniquename limit 100
