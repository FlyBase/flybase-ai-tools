select p.uniquename, p.miniref, p.pyear,
  (select d.accession from pub_dbxref pd join dbxref d on d.dbxref_id = pd.dbxref_id join db on db.db_id = d.db_id
     where pd.pub_id = p.pub_id and lower(db.name) = 'pubmed' limit 1) as pmid
from feature f join feature_pub fp on fp.feature_id = f.feature_id join pub p on p.pub_id = fp.pub_id and p.is_obsolete = false
where f.uniquename = $1 order by p.pyear desc nulls last, p.uniquename limit $2
