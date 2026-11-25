-- $1 include organisms without assemblies, $2 page size (null for all), $3 offset.
with organisms as (
    select o.organism_id, o.abbreviation, o.genus, o.species, o.common_name,
           exists (select 1 from feature f join cvterm t on t.cvterm_id = f.type_id
                   where f.organism_id = o.organism_id and t.name = 'golden_path' and f.is_obsolete = false) as has_assembly
    from organism o
    where o.abbreviation is not null
), totals as (
    select count(*) as organisms_total, count(*) filter (where has_assembly) as assemblies_total
    from organisms
), page as (
    select abbreviation, genus, species, common_name, has_assembly,
           row_number() over (
               order by (abbreviation = 'Dmel') desc, has_assembly desc, genus, species, abbreviation, organism_id
           ) as position
    from organisms
    where $1 or has_assembly
    order by position
    limit $2 offset $3
)
select totals.organisms_total, totals.assemblies_total,
       page.abbreviation, page.genus, page.species, page.common_name, page.has_assembly
from totals left join page on true
order by page.position
