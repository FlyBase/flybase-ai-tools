-- Every non-obsolete feature located on a golden-path arm, flattened with its species, arm, and SO type so a
-- window query reads one narrow table instead of joining feature per located row. Rebuilt on every release load.
-- Computed alignments (type match, about 97% of located rows) are left out: no tool can request that type.
drop table if exists mcp.located_feature;
create table mcp.located_feature as
select o.abbreviation as species, src.uniquename as arm, t.name as type, fl.fmin, fl.fmax, fl.strand,
       f.feature_id, f.uniquename, f.name
from featureloc fl
join feature src on src.feature_id = fl.srcfeature_id
join cvterm st on st.cvterm_id = src.type_id and st.name = 'golden_path'
join organism o on o.organism_id = src.organism_id
join feature f on f.feature_id = fl.feature_id and f.is_obsolete = false
join cvterm t on t.cvterm_id = f.type_id and t.name <> 'match'
where fl.fmin is not null and fl.fmax is not null
order by 1, 2, fl.fmin; -- rows in arm order on disk, so a window's heap reads are local
create index located_feature_window on mcp.located_feature (species, arm, type, fmin, fmax);
create index located_feature_arm on mcp.located_feature (species, arm, fmin);
analyze mcp.located_feature;

-- 007_grants ran before this table existed, so grant it here the same way.
do $$ begin
  if exists (select 1 from pg_roles where rolname = 'flybase_mcp') then
    execute 'grant select on mcp.located_feature to flybase_mcp';
  end if;
end $$;
