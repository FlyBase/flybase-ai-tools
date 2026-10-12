-- $1 FBtp. Alleles associated with the construct and, through gene.allele, the gene each allele belongs to (is_alleleof)
-- or takes its regulatory region from (gene_is_regulatory_region). One row per allele/gene pair.
select al.uniquename as allele_id, al.name as allele_symbol, g.uniquename as gene_id, g.name as gene_symbol,
       a.is_alleleof, a.gene_is_regulatory_region
from feature c join feature_relationship fr on fr.object_id = c.feature_id join cvterm rt on rt.cvterm_id = fr.type_id and rt.name = 'associated_with'
join feature al on al.feature_id = fr.subject_id and al.is_obsolete = false and left(al.uniquename, 4) = 'FBal'
left join gene.allele a on a.fbal_id = al.uniquename left join feature g on g.feature_id = a.gene_id
where c.uniquename = $1 order by al.uniquename, a.is_alleleof desc, g.uniquename limit 500
