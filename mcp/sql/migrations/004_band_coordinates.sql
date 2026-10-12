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

-- Cytological extent of every aberration per arm, from its breakpoints' cyto_left_end/cyto_right_end bands.
drop table if exists mcp.aberration_cyto;
create table mcp.aberration_cyto as
select ab.uniquename as fbab_id, bc.arm, min(bc.fmin) as fmin, max(bc.fmax) as fmax
from feature ab
join feature_relationship br on br.object_id = ab.feature_id
join cvterm brt on brt.cvterm_id = br.type_id and brt.name = 'break_of'
join feature_relationship cr on cr.subject_id = br.subject_id
join cvterm rt on rt.cvterm_id = cr.type_id and rt.name in ('cyto_left_end', 'cyto_right_end')
join feature b on b.feature_id = cr.object_id
join mcp.band_coordinates bc on bc.band = substr(b.name, 6)
where ab.uniquename like 'FBab%' and ab.is_obsolete = false and b.name like 'band-%'
group by ab.uniquename, bc.arm;
create index aberration_cyto_arm on mcp.aberration_cyto (arm, fmin, fmax);
analyze mcp.aberration_cyto;
