-- $1 FBto, $2 fetch. The report includes public symbols and full names,
-- together with the references where each name was reported.
select s.name, t.name as kind, fs.is_current,
       coalesce(array_agg(distinct p.uniquename order by p.uniquename)
          filter (where p.uniquename ~ '^FBrf[0-9]+$'), '{}') as reference_ids
from feature f join feature_synonym fs using (feature_id)
join synonym s using (synonym_id) join cvterm t on t.cvterm_id = s.type_id
join pub p using (pub_id)
where f.uniquename = $1 and not fs.is_internal
group by s.name, t.name, fs.is_current
order by fs.is_current desc, t.name, s.name limit $2
