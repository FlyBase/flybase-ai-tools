-- $1 FBhh. Disease Ontology terms attached to the disease model.
select distinct db.name || ':' || d.accession as term_id, t.name as term
from humanhealth h join humanhealth_cvterm hc on hc.humanhealth_id = h.humanhealth_id join cvterm t on t.cvterm_id = hc.cvterm_id
join dbxref d on d.dbxref_id = t.dbxref_id join db on db.db_id = d.db_id
where h.uniquename = $1 order by term_id limit 50
