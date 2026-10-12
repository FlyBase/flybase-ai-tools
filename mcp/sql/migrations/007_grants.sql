do $$ begin
  if exists (select 1 from pg_roles where rolname = 'flybase_mcp') then
    execute 'grant usage on schema mcp, flybase, gene, gene_group, humanhealth, dataclass, dataclass_relationship, public to flybase_mcp';
    execute 'grant select on all tables in schema mcp, flybase, gene, gene_group, humanhealth, dataclass, dataclass_relationship, public to flybase_mcp';
    execute 'grant execute on all functions in schema flybase, gene, gene_group, humanhealth to flybase_mcp';
  end if;
end $$;
