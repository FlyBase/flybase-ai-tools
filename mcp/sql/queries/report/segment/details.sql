-- Curated size (kb), ends, description, comments, and links shown on the segment report.
select t.name as kind, fp.value,
  array(select distinct p.uniquename from featureprop_pub pp join pub p using (pub_id)
        where pp.featureprop_id = fp.featureprop_id and p.uniquename ~ '^FBrf[0-9]+$'
        order by 1) as references
from feature f join featureprop fp using (feature_id) join cvterm t on t.cvterm_id = fp.type_id
where f.uniquename = $1 and fp.value is not null
  and t.name in ('kb_length','compiled_kb_length','original_left_end','original_right_end',
                 'molecular_info','comment','has_sequence_data')
order by t.name, fp.rank, fp.featureprop_id
limit $2
