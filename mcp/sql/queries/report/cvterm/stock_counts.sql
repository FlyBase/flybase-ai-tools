-- $1 cvterm_id, $2 include descendants. Stock descriptors are recorded as a stock's type, as stock_cvterm
-- annotations, or through collection membership. Chado names provider collections by their vocabulary term.
with terms as (
  select $1::int as cvterm_id
  union select descendant_id from mcp.cvterm_closure where $2::bool and ancestor_id = $1
),
hits as (
  select s.stock_id from stock s join terms t on t.cvterm_id = s.type_id where not s.is_obsolete
  union
  select s.stock_id from stock_cvterm a join terms t using (cvterm_id)
  join stock s using (stock_id) where not s.is_obsolete
  union
  select s.stock_id from stockcollection c
  join stockcollection_stock cs using (stockcollection_id)
  join stock s using (stock_id)
  where not s.is_obsolete and (
    c.type_id in (select cvterm_id from terms)
    or c.uniquename in (
      select ct.name from cvterm ct join terms t using (cvterm_id)
      join cv on cv.cv_id = ct.cv_id where cv.name = 'stock_collection'
    )
  )
)
select 'stock' as class, count(*) as n from hits
