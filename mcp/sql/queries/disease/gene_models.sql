-- $1 FBgn. Disease associations summarised per gene (dataclass_relationship.gene_diseasev2).
select d.id as term_id, d.name as disease, gd.is_experimental, gd.qualifier
from dataclass_relationship.gene_diseasev2 gd join dataclass.diseasev2 d on d.id = gd.disease_id
where gd.gene_id = $1 order by gd.is_experimental desc, d.name limit 500
