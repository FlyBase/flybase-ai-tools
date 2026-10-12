-- $1 text[] of external accessions (UniProt, GenBank, RefSeq, Entrez Gene, HGNC numbers). Current features carrying them.
select x.input, db.name as db, f.uniquename, f.name as symbol
from unnest($1::text[]) x(input)
join dbxref d on d.accession = x.input
join db on db.db_id = d.db_id and db.name in ('UniProt/Swiss-Prot', 'UniProt/TrEMBL', 'UniProt/GCRP', 'GB', 'GB_protein', 'REFSEQ', 'EntrezGene', 'HGNC')
join feature_dbxref fd on fd.dbxref_id = d.dbxref_id and fd.is_current
join feature f on f.feature_id = fd.feature_id and f.is_obsolete = false
-- Genes first so the first surviving ID (and its symbol) is the gene when a UniProt accession also tags polypeptides.
order by x.input, (left(f.uniquename, 4) = 'FBgn') desc, db.name, f.uniquename limit 5000
