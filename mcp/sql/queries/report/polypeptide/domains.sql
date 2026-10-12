-- $1 FBpp, $2 fetch. Polypeptide domains located on the polypeptide (0-based fmin), with their InterPro/Pfam/SMART accessions.
-- Per-exon domain parts (`...-exonN`, partof the whole domain, no dbxrefs; present from FB2026_03) are left out.
select d.name, fl.fmin, fl.fmax,
  (select string_agg(db.name || ':' || x.accession, ' ' order by db.name) from feature_dbxref fd join dbxref x on x.dbxref_id = fd.dbxref_id join db on db.db_id = x.db_id where fd.feature_id = d.feature_id) as accessions
from feature p join featureloc fl on fl.srcfeature_id = p.feature_id join feature d on d.feature_id = fl.feature_id and d.is_obsolete = false
join cvterm dt on dt.cvterm_id = d.type_id and dt.name = 'polypeptide_domain'
where p.uniquename = $1
  and not exists (select 1 from feature_relationship pr join cvterm prt on prt.cvterm_id = pr.type_id and prt.name = 'partof'
    join feature whole on whole.feature_id = pr.object_id and whole.type_id = d.type_id where pr.subject_id = d.feature_id)
order by fl.fmin, fl.fmax, d.name limit $2
