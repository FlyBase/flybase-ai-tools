select gene_id, vector from mcp.rnaseq_matrix where dataset = $1 and stddev > 0 order by gene_id
