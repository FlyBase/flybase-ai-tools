-- $1 text[] of external IDs (UniProt, GenBank, RefSeq, Entrez Gene, HGNC:n). Current features carrying them.
-- $2 is the overflow-checked candidate row limit. HGNC prefixes match only the HGNC database.
with inputs as (
  select input, lower(left(input, 5) collate "C") = 'hgnc:' as hgnc
  from unnest($1::text[]) x(input)
)
select x.input, db.name as db, f.uniquename, f.name as symbol
from inputs x
join dbxref d on d.accession = case when x.hgnc then substr(x.input, 6) else x.input end
join db on db.db_id = d.db_id and db.name in ('UniProt/Swiss-Prot', 'UniProt/TrEMBL', 'UniProt/GCRP', 'GB', 'GB_protein', 'REFSEQ', 'EntrezGene', 'HGNC')
join feature_dbxref fd on fd.dbxref_id = d.dbxref_id and fd.is_current
join feature f on f.feature_id = fd.feature_id and f.is_obsolete = false
where (db.name = 'HGNC') = x.hgnc
-- Genes first so the first surviving ID (and its symbol) is the gene when a UniProt accession also tags polypeptides.
order by x.input, (left(f.uniquename, 4) = 'FBgn') desc, db.name, f.uniquename
limit $2
