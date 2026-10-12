select c.fbal_id as id, array_to_string((array_agg(distinct c.transgenic_product_class order by c.transgenic_product_class))[1:50], '|') as value, count(distinct c.transgenic_product_class) as n
from gene.allele_transgenic_product_class c where c.fbal_id = any($1::text[]) group by c.fbal_id
