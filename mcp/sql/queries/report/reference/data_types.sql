select t.name, count(*) as n from pub p join feature_pub fp on fp.pub_id = p.pub_id join feature f on f.feature_id = fp.feature_id join cvterm t on t.cvterm_id = f.type_id
where p.uniquename = $1 group by t.name order by n desc limit 50
