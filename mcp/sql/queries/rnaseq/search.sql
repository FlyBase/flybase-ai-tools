with cond as (
  select c.sample, c.op, c.v, c.n from unnest($2::text[], $3::text[], $4::real[]) with ordinality as c(sample, op, v, n)
),
hits as (
  select e.gene_id, max(case when c.n = 1 then e.value end) as first_value
  from mcp.rnaseq_expression e
  join cond c on c.sample = e.sample
  where e.dataset = $1
    and case c.op when 'gte' then e.value >= c.v when 'gt' then e.value > c.v
                  when 'lte' then e.value <= c.v when 'lt' then e.value < c.v
                  when 'eq' then e.value = c.v end
  group by e.gene_id
  having count(distinct c.n) = (select count(*) from cond)
),
matched as (
  select h.gene_id, f.name as symbol, h.first_value
  from hits h join feature f on f.uniquename = h.gene_id and f.is_obsolete = false
),
page as (
  select m.* from matched m order by m.first_value desc, m.gene_id limit $5 offset $6
)
-- One row per page gene, or a single all-null row when the page is empty, so total is always present.
select p.gene_id, p.symbol,
       (select json_object_agg(e.sample, e.value order by e.sample) from mcp.rnaseq_expression e
         where e.gene_id = p.gene_id and e.dataset = $1 and e.sample = any($2::text[]))::text as values,
       (select count(*) from matched) as total
from (select 1) as one left join page p on true
order by p.first_value desc nulls last, p.gene_id
