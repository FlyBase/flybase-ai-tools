-- $1 species abbreviation in any case; returns the canonical abbreviation or no row.
select abbreviation from organism where lower(abbreviation) = lower($1) limit 1
