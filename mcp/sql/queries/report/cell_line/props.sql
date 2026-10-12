-- $1 uniquename. Properties shown on the report page (curation bookkeeping excluded).
select pt.name as type, p.value, p.rank
from cell_line x join cell_lineprop p on p.cell_line_id = x.cell_line_id join cvterm pt on pt.cvterm_id = p.type_id
where x.uniquename = $1 and p.value is not null and pt.name not in ('internalnotes', 'owner', 'data_origin', 'fb_ann') and pt.name not like 'derived_stock_%'
order by pt.name, p.rank limit 200
