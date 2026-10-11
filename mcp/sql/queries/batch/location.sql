select f.uniquename as id, src.uniquename || ':' || (fl.fmin + 1) || '..' || fl.fmax || case fl.strand when -1 then '(-)' when 1 then '(+)' else '' end as value, 1::bigint as n
from feature f join featureloc fl on fl.feature_id = f.feature_id and fl.rank = 0 and fl.locgroup = 0
join feature src on src.feature_id = fl.srcfeature_id join cvterm st on st.cvterm_id = src.type_id and st.name = 'golden_path'
where f.uniquename = any($1::text[]) and f.is_obsolete = false and fl.fmin is not null
