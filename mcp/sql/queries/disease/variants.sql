-- $1 FBgn, FBal, or FBhh, $2 fetch. Alleles modelling specific human disease variants (humanhealth.allele_disease_variant).
select v.fbhh_id, v.fbhh_symbol, v.fbgn_id, v.fbgn_symbol, v.fbal_id, v.fbal_symbol, v.div_designation, v.dbxref::text as dbxrefs, v.pubs::text as pubs
from humanhealth.allele_disease_variant v
where $1 in (v.fbgn_id, v.fbal_id, v.fbhh_id) order by v.fbhh_id, v.fbal_id limit $2
