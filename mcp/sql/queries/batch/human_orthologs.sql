select h.gene_id as id, array_to_string((array_agg(split_part(h.name, ';', 1) || ' (HGNC:' || h.accession || ', DIOPT ' || h.diopt_score || ')' order by h.diopt_score desc, h.name))[1:50], '|') as value, count(*) as n
from dataclass_relationship.gene_human_orthologv2 h where h.gene_id = any($1::text[]) group by h.gene_id
