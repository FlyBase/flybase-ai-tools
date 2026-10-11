select f.uniquename as id, array_to_string((array_agg(distinct p.uniquename order by p.uniquename))[1:50], '|') as value, count(distinct p.uniquename) as n
from feature f join feature_pub fp on fp.feature_id = f.feature_id join pub p on p.pub_id = fp.pub_id and p.is_obsolete = false
where f.uniquename = any($1::text[]) and f.is_obsolete = false group by f.uniquename
