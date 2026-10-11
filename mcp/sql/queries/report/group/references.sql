-- $1 FBgg, $2 fetch.
select p.uniquename, p.miniref, p.pyear,
  (select d.accession from pub_dbxref pd join dbxref d on d.dbxref_id = pd.dbxref_id join db on db.db_id = d.db_id where pd.pub_id = p.pub_id and lower(db.name) = 'pubmed' order by d.accession limit 1) as pmid
from grp g join grp_pub gp on gp.grp_id = g.grp_id join pub p on p.pub_id = gp.pub_id and p.is_obsolete = false
where g.uniquename = $1 order by p.pyear desc nulls last, p.uniquename limit $2
