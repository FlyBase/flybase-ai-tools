select f.uniquename as id, f.name as symbol from feature_pub fp join feature f on f.feature_id = fp.feature_id join pub p on p.pub_id = fp.pub_id
where p.uniquename = $1 and f.is_obsolete = false and left(f.uniquename, 4) = 'FBgn' order by f.name limit $2
