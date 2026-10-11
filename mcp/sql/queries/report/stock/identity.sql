select s.uniquename, s.name as stock_number, s.description, g.uniquename as genotype, st.name as stock_type,
  (select sc.uniquename from stockcollection_stock ss join stockcollection sc on sc.stockcollection_id = ss.stockcollection_id where ss.stock_id = s.stock_id limit 1) as provider
from stock s join cvterm st on st.cvterm_id = s.type_id
left join stock_genotype sg on sg.stock_id = s.stock_id left join genotype g on g.genotype_id = sg.genotype_id
where s.uniquename = $1 and s.is_obsolete = false
