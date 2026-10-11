select f.uniquename as id, t.name as value, 1::bigint as n from feature f join cvterm t on t.cvterm_id = f.type_id where f.uniquename = any($1::text[]) and f.is_obsolete = false
