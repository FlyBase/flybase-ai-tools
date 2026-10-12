create schema if not exists mcp;
create table if not exists mcp.migration_log (
  name text primary key, applied_at timestamptz not null default now(), crate_version text not null, release text not null);
