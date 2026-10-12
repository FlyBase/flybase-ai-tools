-- $1 FBgn or FBal, $2 fetch, $3 offset. Disease Ontology annotations on the allele, or on every allele of the gene,
-- with qualifier, evidence, and reference.
with alleles as (
  select f.feature_id from feature f where f.uniquename = $1 and left($1, 4) = 'FBal'
  union
  select a.feature_id from feature g join feature_relationship ar on ar.object_id = g.feature_id join cvterm art on art.cvterm_id = ar.type_id and art.name = 'alleleof'
  join feature a on a.feature_id = ar.subject_id and a.is_obsolete = false where g.uniquename = $1 and left($1, 4) = 'FBgn'
)
select a.uniquename as allele_id, a.name as allele_symbol, db.name || ':' || d.accession as term_id, t.name as disease, fc.is_not,
  (select string_agg(p.value, '; ' order by p.rank) from feature_cvtermprop p join cvterm pt on pt.cvterm_id = p.type_id where p.feature_cvterm_id = fc.feature_cvterm_id and pt.name = 'qualifier') as qualifier,
  (select string_agg(p.value, '; ' order by p.rank) from feature_cvtermprop p join cvterm pt on pt.cvterm_id = p.type_id where p.feature_cvterm_id = fc.feature_cvterm_id and pt.name = 'evidence_code') as evidence,
  pub.uniquename as reference, pub.miniref
from alleles al join feature a on a.feature_id = al.feature_id
join feature_cvterm fc on fc.feature_id = a.feature_id join cvterm t on t.cvterm_id = fc.cvterm_id join cv on cv.cv_id = t.cv_id and cv.name = 'disease_ontology'
join dbxref d on d.dbxref_id = t.dbxref_id join db on db.db_id = d.db_id join pub on pub.pub_id = fc.pub_id
order by t.name, a.name, pub.uniquename limit $2 offset $3
