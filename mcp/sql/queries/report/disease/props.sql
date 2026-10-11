-- $1 FBhh, $2 humanhealthprop type names.
select pt.name as type, p.value, p.rank
from humanhealth h join humanhealthprop p on p.humanhealth_id = h.humanhealth_id join cvterm pt on pt.cvterm_id = p.type_id
where h.uniquename = $1 and pt.name = any($2::text[]) order by pt.name, p.rank limit 200
