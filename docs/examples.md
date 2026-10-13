# Examples

These are questions a biologist might type to an assistant connected to the FlyBase MCP server
(`https://mcp.flybase.org/mcp`). Each one shows the tool call the assistant makes and what comes back.
Every call below was run against the live server on 2026-10-13 (FlyBase release FB2026_03). The IDs and
counts quoted are from those responses. They will drift as FlyBase releases change.

You never have to write these calls yourself. Ask the question in plain language and the assistant picks the
tool. The calls are here so you can see what the server can answer, and so client developers can test it.
Every tool's arguments are listed in [tools.md](tools.md).

Most tools need a FlyBase ID. When you give a symbol or a name, the assistant usually calls `resolve` first and
then passes the ID on. Those extra `resolve` steps are left out below.

## Identify genes, alleles, and other entities

`resolve`, `search`, `validate_ids`, `list_species`

### "What is the FlyBase ID for wingless?"

```json
{"name": "resolve", "arguments": {"query": "wingless"}}
```

Returns ranked matches. The first is gene FBgn0284084, symbol `wg`, matched on its full name.

### "Which fly genes are called insulin-like peptides?"

```json
{"name": "search", "arguments": {"query": "insulin-like peptide", "data_class": "gene", "species": "Dmel", "limit": 10}}
```

Returns hits for Ilp1 to Ilp7, starting with Ilp2 (FBgn0036046). A gene can appear more than once when
several of its synonyms match.

### "Find 2020 papers about Notch signaling."

```json
{"name": "search", "arguments": {"query": "Notch signaling", "data_class": "reference", "year": 2020, "limit": 3}}
```

Returns reference hits with titles and citations, starting with FBrf0244472 (Zhao et al., 2020, Biol. Open),
and `has_more` set for the next page.

### "Which stock is Bloomington 5905?"

```json
{"name": "search", "arguments": {"query": "5905", "data_class": "stock"}}
```

Returns FBst0005905 as the exact match, followed by prefix matches such as 59050.

### "Are these IDs still current: FBgn0000490, FBgn0004644, FBgn0000001, FBgn9999999?"

```json
{"name": "validate_ids", "arguments": {"ids": ["FBgn0000490", "FBgn0004644", "FBgn0000001", "FBgn9999999"]}}
```

Returns one status per input: 3 current (`dpp`, `hh`, `snRNA:4.5S`) and 1 unknown (FBgn9999999).

### "Which Drosophila genomes does FlyBase have?"

```json
{"name": "list_species", "arguments": {}}
```

Returns 12 sequenced species with their abbreviations (Dmel, Dsim, Dpse, ...) and the Muller element
synteny table.

## Reports, batch downloads, and sequences

`get_report`, `batch_download`, `get_sequence`

### "Give me a summary of wingless."

```json
{"name": "get_report", "arguments": {"id": "FBgn0284084"}}
```

Returns the default gene sections: CG4889, cytology 27F1, location 2L:7307159..7316265 (+), a GO summary,
118 orthologs, a phenotype summary, and 807 stocks.

### "List the first five Ubx transcripts and its synonyms."

```json
{"name": "get_report", "arguments": {"id": "FBgn0003944", "sections": ["transcripts", "synonyms"], "limit_per_section": 5}}
```

Returns Ubx-RA (FBtr0083347, 4,282 nt) to Ubx-RE with their polypeptides, `has_more: true`, and the gene's
synonyms. Pass `list_options: true` to see every section a type offers.

### "What kind of allele is w[1118]?"

```json
{"name": "get_report", "arguments": {"id": "FBal0018186"}}
```

Returns the allele summary: gene `w` (FBgn0003996), loss of function allele, spontaneous, 57 papers, a
phenotype summary led by abnormal eye color, and its stock count.

### "What is the genotype of Bloomington stock 5905?"

```json
{"name": "get_report", "arguments": {"id": "FBst0005905"}}
```

Returns the stock's identity and components: genotype `w[1118]`, a living stock at the Bloomington Drosophila
Stock Center, with component allele FBal0018186.

### "Give me symbol, name, location, cytology and CG number for dpp, hh, wg and Ubx as a table."

```json
{"name": "batch_download", "arguments": {"ids": ["FBgn0000490", "FBgn0004644", "FBgn0284084", "FBgn0003944"], "fields": ["symbol", "name", "location", "cytology", "annotation_id"], "format": "tsv"}}
```

Returns a 4-row TSV block, for example `hh  hedgehog  3R:23128169..23141906(-)  94E1-94E1  CG4637`. Call it
with `"list_options": true` (and empty `ids` and `fields`) to see the field catalogue by class.

### "Get me the Wingless protein sequence."

```json
{"name": "get_sequence", "arguments": {"id": "FBgn0284084", "kind": "protein"}}
```

Returns one FASTA record per isoform. Here there is one: wg-RA, 468 residues, starting `MDISYIFVICLMALCSGG`.

### "I need 2 kb upstream of hedgehog for a reporter construct."

```json
{"name": "get_sequence", "arguments": {"id": "FBgn0004644", "kind": "upstream", "padding": 2000}}
```

Returns 2,000 bp from 3R:23141907..23143906 on the minus strand, the gene's own orientation.

### "What is the sequence of 2L:7307159-7307300?"

```json
{"name": "get_sequence", "arguments": {"region": {"chromosome": "2L", "start": 7307159, "end": 7307300}}}
```

Returns 142 bp of FASTA, starting `TCAGTTAAGCGTTGGCACTG`.

## The genome: features, cytology, and coordinates

`map_features`, `cytosearch`, `convert_coordinates`

### "What genes are near wingless, in 2L:7,290,000-7,340,000?"

```json
{"name": "map_features", "arguments": {"region": {"chromosome": "2L", "start": 7290000, "end": 7340000}, "feature_types": ["gene"]}}
```

Returns 3 genes with coordinates and overlap: `wg` and `lncRNA:CR45304` within the range, and `Wnt6`
(FBgn0031902) partly overlapping.

### "Which transposon insertions are within 10 kb of hedgehog?"

```json
{"name": "map_features", "arguments": {"id": "FBgn0004644", "padding": 10000, "feature_types": ["insertion"], "limit": 5}}
```

Returns the first 5 insertions in 3R:23118169..23151906, such as P{EPgy2}EY09013 (FBti0038879), with
`has_more: true`.

### "What genes and aberrations are in cytological band 89D?"

```json
{"name": "cytosearch", "arguments": {"location": "89D", "limit": 5}}
```

Returns the resolved range 3R:16576249..16736155, the first 5 features (starting with `Scp2`), and the first 5
aberrations with mapped breakpoints, such as In(3R)Ubx[125].

### "Which genes are in 27F1? Skip the aberrations."

```json
{"name": "cytosearch", "arguments": {"location": "27F1", "include_aberrations": false, "feature_types": ["gene"]}}
```

Returns 3 genes in 2L:7277617..7334242: `wg`, `lncRNA:CR45304`, and `Wnt6`.

### "A 2012 paper gives 2L:21485500..21485700, X:330 and 3R:12000000. What are they in release 6?"

```json
{"name": "convert_coordinates", "arguments": {"coordinates": ["2L:21485500..21485700", "X:330", "3R:12000000"]}}
```

Returns all 3 as mapped. X:330 becomes X:103943 and 3R:12000000 becomes 3R:16174278. The 2L range keeps its
length but is flagged as spanning 1 region of change.

## Controlled vocabularies

`search_vocabulary`, `get_term`, `entities_for_term`

### "What is the anatomy term for the mushroom body?"

```json
{"name": "search_vocabulary", "arguments": {"query": "mushroom body", "limit": 5}}
```

Returns FBbt:00005801 (mushroom body) with its definition, then related terms.

### "What is the Disease Ontology ID for Parkinson's disease?"

```json
{"name": "search_vocabulary", "arguments": {"query": "Parkinson disease", "vocabularies": ["disease"], "limit": 3}}
```

Returns DOID:14330 (Parkinson's disease), then narrower diseases such as Kufor-Rakeb syndrome
(DOID:0060556). The vocabulary names are `go`, `anatomy`, `development`, `disease`, `sequence`, `misc`, and
`psi_mi`.

### "Tell me about GO:0007219."

```json
{"name": "get_term", "arguments": {"term_id": "GO:0007219"}}
```

Returns Notch signaling pathway with its definition, parents, 7 child terms (for example GO:0045747, positive
regulation), and annotation counts: 27 genes and 4 gene groups.

### "Which genes are annotated to Notch signaling?"

```json
{"name": "entities_for_term", "arguments": {"term_id": "GO:0007219", "data_class": "gene", "limit": 5}}
```

Returns genes with evidence summaries, including the term's 5 descendants: Delta (FBgn0000463), N, Su(H),
Ser, and kuz, with more by offset.

### "Which alleles are used to model Parkinson's disease?"

```json
{"name": "entities_for_term", "arguments": {"term_id": "DOID:14330", "data_class": "allele", "limit": 5}}
```

Returns alleles with qualifiers and reference counts across 21 descendant terms, starting with Pink1[B9]
(FBal0193144) and including park[25] and human SNCA transgenes.

### "Which alleles have a phenotype in the mushroom body itself?"

```json
{"name": "entities_for_term", "arguments": {"term_id": "FBbt:00005801", "data_class": "allele", "include_descendants": false, "limit": 5}}
```

Returns alleles with phenotype statements on that term, led by the driver Scer\GAL4[ey-OK107]
(FBal0058772, 20 statements).

## Homepage data: orthologs, interactions, groups, disease, stocks

`get_orthologs`, `get_interactions`, `get_gene_groups_and_pathways`, `get_disease_models`, `get_stocks`

### "What is the best human ortholog of wingless?"

```json
{"name": "get_orthologs", "arguments": {"id": "FBgn0284084", "species": ["Hsap"], "best_only": true}}
```

Returns 2 best-scoring human genes. WNT1 (HGNC:12774) has DIOPT score 12 and is best in both directions, with
its OMIM phenotypes. WNT3 scores 3.

### "Find mouse and zebrafish orthologs of white."

```json
{"name": "get_orthologs", "arguments": {"id": "FBgn0003996", "species": ["Mmus", "Drer"], "limit": 4}}
```

Returns 15 orthologs in total, ranked by score, starting with Drer\abcg2a and Mmus\Abcg2 at score 9. The
`species` argument is a list, even for one species.

### "Which proteins physically interact with Notch?"

```json
{"name": "get_interactions", "arguments": {"id": "FBgn0004647", "kind": "physical", "limit": 5}}
```

Returns physical interactions with partner, role, method, and reference. The first is Abl (FBgn0000017),
found by co-immunoprecipitation and western blot (FBrf0255228).

### "What genetic interactions are known for Ubx?"

```json
{"name": "get_interactions", "arguments": {"id": "FBgn0003944", "kind": "genetic", "limit": 3}}
```

Returns genetic interactions with partner genes and references. The first partner is Abd-B (FBgn0000015).

### "Which pathways and gene groups is wingless in?"

```json
{"name": "get_gene_groups_and_pathways", "arguments": {"id": "FBgn0284084"}}
```

Returns 2 gene groups (WNTs, RECEPTOR LIGANDS) and 2 pathways, including WNT-TCF SIGNALING PATHWAY
(FBgg0000889).

### "Is Notch part of a complex?"

```json
{"name": "get_gene_groups_and_pathways", "arguments": {"id": "FBgn0004647"}}
```

Returns 3 gene groups, including CSL-NOTCH-MASTERMIND TRANSCRIPTION FACTOR COMPLEX (FBgg0001066), and
2 pathways, including NOTCH SIGNALING PATHWAY (FBgg0001068).

### "What human diseases is Pink1 used to model?"

```json
{"name": "get_disease_models", "arguments": {"id": "FBgn0029891", "limit": 5}}
```

Returns 7 disease associations, including Parkinson's disease (DOID:14330) and Parkinson's disease 6, plus
allele-level annotations, 4 disease variant alleles, and 1 FlyBase human disease model entry.

### "Is Drp1 linked to any disease?"

```json
{"name": "get_disease_models", "arguments": {"id": "FBgn0026479", "limit": 5}}
```

Returns 3 associations, including encephalopathy due to defective mitochondrial and peroxisomal fission 1
(DOID:0070347), and the disease model report FBhh0000619.

### "I want a GAL4 driver for wingless. What can I order?"

```json
{"name": "get_stocks", "arguments": {"id": "FBgn0284084", "gal4_only": true, "limit": 3}}
```

Returns 3 stocks of Scer\GAL4[wg.PM], such as Bloomington 4918 (FBst0004918, `w[*]; P{GAL4-wg.M}MA1`)
and Kyoto 101852.

### "Which Bloomington stocks carry w[1118]?"

```json
{"name": "get_stocks", "arguments": {"id": "FBal0018186", "provider": "bloomington", "limit": 3}}
```

Returns Bloomington stocks with genotypes, starting with stock 6 (FBst0000006), with more by offset.
Provider names are lower case: `bloomington`, `kyoto`, `vdrc`, `nig_fly`, `korea`, `flyorf`, `ndssc`.

### "Which stocks carry the FM7c balancer?"

```json
{"name": "get_stocks", "arguments": {"id": "FM7c", "limit": 3}}
```

Resolves FM7c to FBba0000009 and returns its stocks, starting with Bloomington 616 (FBst0000616).

## RNA-seq expression

`rnaseq_profile_search`, `rnaseq_similar_genes`

### "Which RNA-seq datasets and samples can I filter on?"

```json
{"name": "rnaseq_profile_search", "arguments": {"list_options": true}}
```

Returns 5 datasets with their sample names: FlyAtlas2 (43 samples), modENCODE_development (30),
modENCODE_tissues (29), modENCODE_treatments (27), and modENCODE_cell_lines (24).

### "Which genes are highly expressed in adult testis but off in the ovary?"

```json
{"name": "rnaseq_profile_search", "arguments": {"conditions": [{"dataset": "FlyAtlas2", "sample": "Adult_Male_Testis", "operator": "gte", "value": 500}, {"dataset": "FlyAtlas2", "sample": "Adult_Female_Ovary", "operator": "lte", "value": 1}], "limit": 5}}
```

Returns 194 matching genes ordered by testis RPKM, starting with Met75Ca (FBgn0028416) at 4,105 RPKM in testis
and 0 in ovary.

### "Which genes are high in early embryos and gone by mid-embryogenesis?"

```json
{"name": "rnaseq_profile_search", "arguments": {"conditions": [{"dataset": "modENCODE_development", "sample": "em0-2hr", "operator": "gte", "value": 200}, {"dataset": "modENCODE_development", "sample": "em14-16hr", "operator": "lte", "value": 5}], "limit": 5}}
```

Returns 12 genes, mostly maternal transcripts, including `osk` (FBgn0003015), `BicC`, and `mtrm`.

### "Which genes are co-expressed with oskar during development?"

```json
{"name": "rnaseq_similar_genes", "arguments": {"id": "FBgn0003015", "dataset": "modENCODE_development", "limit": 5}}
```

Returns genes ranked by Pearson correlation over 30 samples and 15,780 compared genes. The top hit is `yl`
(FBgn0004649) at r = 0.9899.

### "Which genes follow period's tissue profile closely, with r of 0.85 or more?"

```json
{"name": "rnaseq_similar_genes", "arguments": {"id": "FBgn0003068", "dataset": "FlyAtlas2", "min_correlation": 0.85, "limit": 5}}
```

Returns 1 gene above the cut-off: Eip93F (FBgn0264490) at r = 0.8872. When nothing passes, the answer says so
and the list is empty.

## BLAST

`blast_submit`, `blast_status`, `blast_results`

BLAST runs as a job. The assistant submits, waits, polls, and fetches results. The databases are the archived
FB2023_04 set, so results can lag the current FlyBase release, and every submit response says so.

### "What can I BLAST against?"

```json
{"name": "blast_submit", "arguments": {"list_options": true}}
```

Returns 5 programs (blastn, blastp, blastx, tblastn, tblastx), 11 databases (assembly, transcript,
translation, ...), 12 species, and 5 scoring matrices. Not every species has every database. For example a
`translation` search for Dpse is rejected with `invalid species`.

### "BLAST this protein fragment against D. melanogaster polypeptides and tell me what it is: MDISYIFVICLMALCSGG...KNLFGKIV"

```json
{"name": "blast_submit", "arguments": {"program": "blastp", "database": "translation", "sequence": "MDISYIFVICLMALCSGGSSLSQVEGKQKSGRGRGSMWWGIAKVGEPNNITPIMYMDPAIHSTLRRKQRRLVRDNPGVLGALVKGANLAISECQHQFRNRRWNCSTRNFSRGKNLFGKIV"}}
```

Returns a `job_id`, a `job_token`, the queue state, and `poll_after_seconds: 5`. Then:

```json
{"name": "blast_status", "arguments": {"job_id": "<job_id>", "job_token": "<job_token>"}}
```

Returns `state: complete` once the job is done (`queued` or `running` before that). Then:

```json
{"name": "blast_results", "arguments": {"job_id": "<job_id>", "job_token": "<job_token>", "max_hits": 5, "max_hsps_per_hit": 1}}
```

Returns 5 of 16 hits. The best is FBpp0079060 (wg-PA) at 100% identity, e-value 6.21e-83 and bit score 250.

### "Where in the genome does this 142 bp sequence come from?"

```json
{"name": "blast_submit", "arguments": {"program": "blastn", "database": "assembly", "sequence": "TCAGTTAAGCGTTGGCACTG...", "megablast": true}}
```

After `blast_status` and `blast_results`, returns 1 hit on chromosome arm 2L (`ID=2L` in the hit description)
at 7307159..7307300, 100% identity, e-value 9.01e-70. Genome hits carry no FlyBase ID.

### "Is this Wingless fragment conserved in D. pseudoobscura and D. virilis?"

```json
{"name": "blast_submit", "arguments": {"program": "tblastn", "database": "assembly", "sequence": "MDISYIFVICLMALCSGG...KNLFGKIV", "species": ["Dmel", "Dpse", "Dvir"]}}
```

After `blast_status` and `blast_results`, returns 13 hits across the three genomes: Dmel at 100% identity
(e-value 4e-49), Dpse at 88.8% and Dvir at 91.8%.

The sequences in the last two calls are shortened here; the calls that were run used the full 142 bp and
120-residue queries shown earlier. Treat a `job_token` as a secret: with the job ID it reads that job's
results, so do not paste it anywhere public.
