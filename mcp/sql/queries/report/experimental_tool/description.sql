-- $1 FBto, $2 fetch. Report description and notes with their annotation citations.
select pt.name as kind, fp.value as text,
  array(select distinct p.uniquename
        from featureprop_pub pp join pub p using (pub_id)
        where pp.featureprop_id = fp.featureprop_id and p.uniquename ~ '^FBrf[0-9]+$'
        order by p.uniquename) as reference_ids
from feature f join featureprop fp using (feature_id)
join cvterm pt on pt.cvterm_id = fp.type_id
where f.uniquename = $1 and pt.name in ('description', 'misc') and nullif(fp.value, '') is not null
order by pt.name, fp.rank, fp.featureprop_id limit $2
