select p.uniquename, p.miniref, p.pyear,
  case when f.uniquename like 'FBgn%' then p.is_obsolete end as is_obsolete,
  (select d.accession from pub_dbxref pd join dbxref d on d.dbxref_id = pd.dbxref_id join db on db.db_id = d.db_id
     where pd.pub_id = p.pub_id and lower(db.name) = 'pubmed' order by d.accession limit 1) as pmid
from feature f cross join lateral mcp.feature_publications(f.feature_id, f.uniquename like 'FBgn%') refs join pub p on p.pub_id = refs.pub_id
where f.uniquename = $1 order by p.pyear desc nulls last, p.uniquename limit $2 offset $3
