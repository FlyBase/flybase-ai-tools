select f.uniquename as id, o.abbreviation as value, 1::bigint as n from feature f join organism o on o.organism_id = f.organism_id where f.uniquename = any($1::text[]) and f.is_obsolete = false
