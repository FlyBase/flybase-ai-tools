drop table if exists mcp.band_coordinates;
create table mcp.band_coordinates as
select regexp_replace(f.name, '^band-', '') as band, src.uniquename as arm, fl.fmin, fl.fmax,
       case when f.name ~ '^band-[0-9]+[A-H][0-9]+$' then 'band'
            when f.name ~ '^band-[0-9]+[A-H]$' then 'subdivision'
            when f.name ~ '^band-[0-9]+$' then 'division'
            else 'other' end as level
from feature f join cvterm t on t.cvterm_id = f.type_id and t.name = 'chromosome_band'
join featureloc fl on fl.feature_id = f.feature_id join feature src on src.feature_id = fl.srcfeature_id
join organism o on o.organism_id = f.organism_id and o.abbreviation = 'Dmel'
where f.is_obsolete = false;
create index band_coordinates_band on mcp.band_coordinates (band);
analyze mcp.band_coordinates;
