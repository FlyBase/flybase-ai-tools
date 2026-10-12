-- Earlier builds created pg_trgm inside the droppable mcp schema. Move it to public:
-- the cascade drops the trigram indexes, which 002 and 003 rebuild.
do $$
begin
  if exists (select 1 from pg_extension e join pg_namespace n on n.oid = e.extnamespace
             where e.extname = 'pg_trgm' and n.nspname = 'mcp') then
    drop extension if exists pg_trgm cascade;
  end if;
end $$;
create extension if not exists pg_trgm with schema public;
