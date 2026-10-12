-- $2 GO aspect cv name: biological_process, molecular_function, or cellular_component.
select f.uniquename as id, array_to_string((array_agg(distinct 'GO:' || d.accession || ' ' || t.name order by 'GO:' || d.accession || ' ' || t.name))[1:50], '|') as value, count(distinct t.cvterm_id) as n
from feature f join feature_cvterm fc on fc.feature_id = f.feature_id and fc.is_not = false
join cvterm t on t.cvterm_id = fc.cvterm_id join cv c on c.cv_id = t.cv_id and c.name = $2 join dbxref d on d.dbxref_id = t.dbxref_id
where f.uniquename = any($1::text[]) and f.is_obsolete = false group by f.uniquename
