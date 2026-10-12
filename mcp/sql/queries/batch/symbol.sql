select f.uniquename as id, f.name as value, 1::bigint as n from feature f where f.uniquename = any($1::text[]) and f.is_obsolete = false
