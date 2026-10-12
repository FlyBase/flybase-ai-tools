-- $1 band, subdivision, or division name (91A1, 91A, 91). Coordinates from the chromosome_band features (0-based fmin).
select band, arm, fmin, fmax, level from mcp.band_coordinates where band = $1 order by level, fmin limit 1
