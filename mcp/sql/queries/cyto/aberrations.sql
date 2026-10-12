-- $1 arm, $2 fmin, $3 fmax, $4 fetch, $5 offset. Aberrations whose cytological extent (from breakpoint bands, migration 004) overlaps the range.
select a.fbab_id as id, f.name as symbol, a.fmin, a.fmax,
  case when a.fmin >= $2 and a.fmax <= $3 then 'within' when a.fmin <= $2 and a.fmax >= $3 then 'contains' else 'partial' end as overlap
from mcp.aberration_cyto a join feature f on f.uniquename = a.fbab_id
where a.arm = $1 and a.fmax > $2 and a.fmin < $3
order by a.fmin, a.fmax, a.fbab_id limit $4 offset $5
