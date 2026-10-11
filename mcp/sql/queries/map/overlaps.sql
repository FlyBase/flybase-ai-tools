-- $1 species, $2 chromosome arm, $3 fmin (0-based), $4 fmax, $5 SO type names, $6 fetch, $7 offset.
-- Features on the golden path overlapping the range, with the overlap kind relative to the query range.
select f.uniquename as id, f.name as symbol, t.name as type, fl.fmin, fl.fmax, fl.strand,
  case when fl.fmin >= $3 and fl.fmax <= $4 then 'within' when fl.fmin <= $3 and fl.fmax >= $4 then 'contains' else 'partial' end as overlap
from feature src join organism o on o.organism_id = src.organism_id and o.abbreviation = $1
join cvterm st on st.cvterm_id = src.type_id and st.name = 'golden_path'
join featureloc fl on fl.srcfeature_id = src.feature_id and fl.fmax > $3 and fl.fmin < $4
join feature f on f.feature_id = fl.feature_id and f.is_obsolete = false
join cvterm t on t.cvterm_id = f.type_id and t.name = any($5::text[])
where src.uniquename = $2
order by fl.fmin, fl.fmax, f.uniquename limit $6 offset $7
