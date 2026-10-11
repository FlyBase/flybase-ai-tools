-- $1 a human gene symbol (NOTCH1) or HGNC accession (7881, from "HGNC:7881"). The Hsap ortholog features that match.
select f.uniquename, f.name as symbol
from feature f join organism o on o.organism_id = f.organism_id and o.abbreviation = 'Hsap'
where f.is_obsolete = false and (
  lower(f.name) = lower('Hsap\' || $1)
  or exists (select 1 from feature_dbxref fd join dbxref d on d.dbxref_id = fd.dbxref_id join db on db.db_id = d.db_id
             where fd.feature_id = f.feature_id and fd.is_current and db.name = 'HGNC' and d.accession = $1))
order by f.uniquename limit 10
