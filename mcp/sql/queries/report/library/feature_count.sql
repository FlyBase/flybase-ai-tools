-- $1 FBlc. Number of features linked to the library (for the identity section and has_more arithmetic).
select count(*)::bigint as n from library x join library_feature xf on xf.library_id = x.library_id where x.uniquename = $1
