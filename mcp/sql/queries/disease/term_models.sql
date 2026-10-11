-- $1 DOID accession (digits), $2 fetch, $3 offset. Disease model entries annotated to the DO term or its descendants.
with t as (select c.cvterm_id, c.name from cvterm c join dbxref d on d.dbxref_id = c.dbxref_id join db on db.db_id = d.db_id where db.name = 'DOID' and d.accession = $1),
terms as (select cvterm_id from t union select descendant_id from mcp.cvterm_closure cc join t on t.cvterm_id = cc.ancestor_id)
select h.uniquename as id, h.name
from humanhealth_cvterm hc join terms on terms.cvterm_id = hc.cvterm_id join humanhealth h on h.humanhealth_id = hc.humanhealth_id and h.is_obsolete = false
group by h.humanhealth_id, h.uniquename, h.name order by h.name limit $2 offset $3
