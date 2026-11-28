select f.uniquename as id, array_to_string((array_agg(distinct p.uniquename order by p.uniquename))[1:50], '|') as value, count(distinct p.uniquename) as n
from feature f cross join lateral mcp.feature_publications(f.feature_id, f.uniquename like 'FBgn%') refs join pub p on p.pub_id = refs.pub_id
where f.uniquename = any($1::text[]) and f.is_obsolete = false group by f.uniquename
