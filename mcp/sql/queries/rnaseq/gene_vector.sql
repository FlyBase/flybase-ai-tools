select array_length(vector, 1) as n, mean, stddev from mcp.rnaseq_matrix where dataset = $1 and gene_id = $2
