-- The current segment is the container. Each part keeps its rank and relative orientation.
select r.rank, s.uniquename as id, s.name as symbol,
  array(select rp.value from feature_relationshipprop rp join cvterm t on t.cvterm_id = rp.type_id
        where rp.feature_relationship_id = r.feature_relationship_id and t.name = 'relative_orientation'
          and rp.value is not null order by rp.rank, rp.feature_relationshipprop_id) as orientations,
  array(select distinct p.uniquename from feature_relationship_pub rp join pub p using (pub_id)
        where rp.feature_relationship_id = r.feature_relationship_id and p.uniquename ~ '^FBrf[0-9]+$'
        order by 1) as references
from feature f join feature_relationship r on r.object_id = f.feature_id
join cvterm t on t.cvterm_id = r.type_id and t.name = 'partof'
join feature s on s.feature_id = r.subject_id and not s.is_obsolete
where f.uniquename = $1
order by r.rank, s.uniquename, r.feature_relationship_id limit $2
