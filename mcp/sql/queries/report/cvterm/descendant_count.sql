-- $1 cvterm_id. Number of is_a/part_of descendants from the closure table.
select count(*)::bigint as n from mcp.cvterm_closure where ancestor_id = $1
