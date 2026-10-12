select uniquename, name from feature where uniquename = any($1::text[]) and is_obsolete = false
