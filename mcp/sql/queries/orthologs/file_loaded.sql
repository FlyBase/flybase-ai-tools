-- Whether `flybase-mcp migrate --diopt` has loaded the human ortholog file (migration 006).
select exists (select 1 from mcp.diopt_orthologs) as loaded
