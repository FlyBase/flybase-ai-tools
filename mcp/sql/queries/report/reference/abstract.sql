select pp.value from pubprop pp join cvterm t on t.cvterm_id = pp.type_id and t.name = 'pubmed_abstract' join pub p on p.pub_id = pp.pub_id where p.uniquename = $1 limit 1
