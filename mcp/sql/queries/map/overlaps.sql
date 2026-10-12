-- $1 species, $2 chromosome arm, $3 fmin (0-based), $4 fmax, $5 SO type names, $6 fetch, $7 offset.
-- Features on the golden path overlapping the range, with the overlap kind relative to the query range.
-- Reads mcp.located_feature (migration 008), which already holds species, arm, type, and is_obsolete = false.
select lf.uniquename as id, lf.name as symbol, lf.type, lf.fmin, lf.fmax, lf.strand,
  case when lf.fmin >= $3 and lf.fmax <= $4 then 'within' when lf.fmin <= $3 and lf.fmax >= $4 then 'contains' else 'partial' end as overlap
from mcp.located_feature lf
where lf.species = $1 and lf.arm = $2 and lf.fmax > $3 and lf.fmin < $4 and lf.type = any($5::text[])
order by lf.fmin, lf.fmax, lf.uniquename limit $6 offset $7
