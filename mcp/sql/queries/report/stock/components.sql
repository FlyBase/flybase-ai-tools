select f.uniquename as id, f.name as symbol, t.name as type from stock s join stock_genotype sg on sg.stock_id = s.stock_id
join feature_genotype fg on fg.genotype_id = sg.genotype_id join feature f on f.feature_id = fg.feature_id join cvterm t on t.cvterm_id = f.type_id
where s.uniquename = $1 order by f.uniquename limit 200
