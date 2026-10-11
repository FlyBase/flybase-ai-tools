-- $1 FBlc. Number of current (non-obsolete) features linked to the library, matching what features.sql can list.
select count(*)::bigint as n
from library x join library_feature xf on xf.library_id = x.library_id join feature f on f.feature_id = xf.feature_id and f.is_obsolete = false
where x.uniquename = $1
