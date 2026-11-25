# FlyBase website parity checks

The MCP integration tests call the registered tools over HTTP against the [Chado fixture](../fixture/README.md).
The result checks below use stable IDs, fields, and values from FlyBase reports and tools. The report, sequence,
and expression examples use FB2026_03. Search order, record counts, and fields available in the fixture can vary
with the release and fixture seed set. The tests exercise representative records and selected response contracts.

Run the suite from the repository root with a disposable fixture database loaded:

```sh
CHADO_URL=postgres://flybase_mcp:flybase_mcp@127.0.0.1:5499/flybase \
  cargo test --locked --manifest-path mcp/Cargo.toml --tests
```

| MCP tool | Website reference and result checked | Integration test |
|---|---|---|
| `resolve` | [Jump to Gene](https://flybase.org/) and the [Notch report](https://flybase.org/reports/FBgn0004647). Symbol `N` resolves to `FBgn0004647`, and secondary ID `FBgn0000311` points to the same record. | [tools_resolve.rs](../mcp/tests/tools_resolve.rs) |
| `search` | [QuickSearch](https://flybase.org/) and the [Notch report](https://flybase.org/reports/FBgn0004647). A name match carries the Notch gene ID, current symbol, species, and report link. | [tools_search.rs](../mcp/tests/tools_search.rs) |
| `validate_ids` | [ID Validator](https://flybase.org/convert/id) and the [Notch report](https://flybase.org/reports/FBgn0004647). Representative current, secondary, external, ambiguous, and unknown inputs have per-input outcomes. | [tools_validate.rs](../mcp/tests/tools_validate.rs) |
| `list_species` | [Sequenced Species](https://flybase.org/static/sequenced_species) and the [Muller element table](https://flybase.org/maps/synteny). The default catalog includes Dmel and the expanded catalog includes organisms without assemblies. | [conformance.rs](../mcp/tests/conformance.rs), [species.rs](../mcp/src/tools/species.rs) |
| `get_report` | [Notch gene report](https://flybase.org/reports/FBgn0004647). Symbol `N`, name Notch, protein-coding type, and location X:3134870..3172221 on the plus strand. Additional tests cover other report classes. | [tools_report_gene.rs](../mcp/tests/tools_report_gene.rs) and the other `tools_report_*.rs` files |
| `batch_download` | [Batch Download](https://flybase.org/batchdownload) and the [Notch](https://flybase.org/reports/FBgn0004647) and [dpp](https://flybase.org/reports/FBgn0000490) reports. The symbols, names, and annotation IDs are preserved across TSV pages, with one row per input in order, including a repeat. | [tools_batch.rs](../mcp/tests/tools_batch.rs) |
| `get_sequence` | Sequence Downloader FASTA for [X:3134870..3134969](https://flybase.org/api/sequence/region/dmel/X%3A3134870..3134969?format=fasta), [FBtr0070507](https://flybase.org/api/sequence/id/FBtr0070507?format=fasta), and [FBpp0071769](https://flybase.org/api/sequence/id/FBpp0071769?format=fasta). Compares all residues directly or by SHA-256 over the unwrapped residues. | [tools_sequence.rs](../mcp/tests/tools_sequence.rs) |
| `map_features` | [Feature Mapper](https://flybase.org/featuremapper) and the [Notch report](https://flybase.org/reports/FBgn0004647). Notch falls within its reported X span, with inclusive endpoints and plus strand. | [tools_map.rs](../mcp/tests/tools_map.rs) |
| `cytosearch` | [CytoSearch](https://flybase.org/cytosearch) and the computed cytological location on the [Notch report](https://flybase.org/reports/FBgn0004647). Notch overlaps 3C7 partially and is excluded when partial matches are disabled. | [tools_map.rs](../mcp/tests/tools_map.rs) |
| `convert_coordinates` | [Coordinates Converter](https://flybase.org/convert/coordinates). Checks mapped, flipped, partial, unmapped, and invalid inputs. A separate comparison uses the published release 5 and release 6 anchors. | [tools_coordinates.rs](../mcp/tests/tools_coordinates.rs), [coordinate_parity.sh](../scripts/coordinate_parity.sh) |
| `search_vocabulary` | [Vocabularies](https://flybase.org/vocabularies) and the [Notch signaling pathway term](https://flybase.org/cgi-bin/cvreport.pl?id=GO:0007219). The term appears for a name search and carries its synonym. | [tools_vocabulary.rs](../mcp/tests/tools_vocabulary.rs) |
| `get_term` | [Notch signaling pathway term report](https://flybase.org/cgi-bin/cvreport.pl?id=GO:0007219). Stable term ID, term name, vocabulary, and annotation counts. | [tools_vocabulary.rs](../mcp/tests/tools_vocabulary.rs) |
| `entities_for_term` | Notch gene annotations on the [gene report](https://flybase.org/reports/FBgn0004647). The `GO:0007219` results include Notch with evidence and references. | [tools_entities.rs](../mcp/tests/tools_entities.rs) |
| `get_orthologs` | Human orthologs on the [Notch report](https://flybase.org/reports/FBgn0004647). The DIOPT-filtered result includes NOTCH1 with HGNC:7881, score, and best-score flags. | [tools_orthologs.rs](../mcp/tests/tools_orthologs.rs) |
| `get_interactions` | Interactions on the [Notch report](https://flybase.org/reports/FBgn0004647). Physical association with Abl and a genetic interaction with A122, with supporting references. | [tools_interactions.rs](../mcp/tests/tools_interactions.rs) |
| `get_gene_groups_and_pathways` | [Notch report](https://flybase.org/reports/FBgn0004647) and [complex report](https://flybase.org/reports/FBgg0001066.html). Distinguishes the Notch core pathway from the CSL-NOTCH-MASTERMIND complex. | [tools_homepage.rs](../mcp/tests/tools_homepage.rs) |
| `get_disease_models` | [NOTCH-SCRIB-related epithelial cancer model](https://flybase.org/reports/FBhh0000679.html). The query for Notch returns the model ID and the implicated fly-gene role. | [tools_homepage.rs](../mcp/tests/tools_homepage.rs) |
| `get_stocks` | [Bloomington stock 28813](https://flybase.org/reports/FBst0028813.html). Returns the stock from its linked Notch allele, with genotype, collection and stock number. | [tools_homepage.rs](../mcp/tests/tools_homepage.rs) |
| `rnaseq_profile_search` | [RNA-Seq Profile Search](https://flybase.org/rnaseq/profile_search) and [Notch FlyAtlas2 data](https://flybase.org/expression/ht_data/FBgn0004647/FlyAtlas2). The threshold result for Notch reports 64 RPKM in adult female ovary and 57 in larval central nervous system. | [tools_rnaseq.rs](../mcp/tests/tools_rnaseq.rs) |
| `rnaseq_similar_genes` | [RNA-Seq Similarity](https://flybase.org/rnaseq/simsearch) and the FlyAtlas2 profiles for [Notch](https://flybase.org/expression/ht_data/FBgn0004647/FlyAtlas2) and [CG4612](https://flybase.org/expression/ht_data/FBgn0035016/FlyAtlas2). Test checks the Pearson correlation of 0.8724 from applying the MCP transform to those public values. | [tools_rnaseq.rs](../mcp/tests/tools_rnaseq.rs) |
| `blast_submit` | Fixture job accepts a valid program, database, and query, returning a job ID, polling delay, and access token. | [tools_blast.rs](../mcp/tests/tools_blast.rs), [real_blast.rs](../blast-jobs/tests/real_blast.rs) |
| `blast_status` | Fixture job progresses to completion and returns the result page and downloadable formats. | [tools_blast.rs](../mcp/tests/tools_blast.rs), [real_blast.rs](../blast-jobs/tests/real_blast.rs) |
| `blast_results` | The fixture preserves hit IDs, HSP coordinates, E-values, and bit scores. The real BLAST fixture matches Notch transcript and protein queries to their archived subject IDs. | [tools_blast.rs](../mcp/tests/tools_blast.rs), [real_blast.rs](../blast-jobs/tests/real_blast.rs) |
| `search_wiki` | [FlyBase MediaWiki](https://wiki.flybase.org/) search snapshots. Checks the order and canonical URLs of stock-related hits, redirect resolution, and snippet cleanup. | [tools_wiki.rs](../mcp/tests/tools_wiki.rs), [snapshot provenance](../mcp/tests/fixtures/wiki/README.md) |
| `get_wiki_page` | [Aberration Report help](https://wiki.flybase.org/wiki/FlyBase:Aberration_Report) parse snapshot. Checks canonical title, URL, revision, section headings, and plain-text conversion. | [tools_wiki.rs](../mcp/tests/tools_wiki.rs), [snapshot provenance](../mcp/tests/fixtures/wiki/README.md) |

## Differences in behavior

- QuickSearch offers website-specific search modes and filters. MCP search uses literal matching with deterministic ranking, and resolve provides exact identifiers and name candidates.
- The website species pages describe assembled species and show a separate synteny table. MCP can also return the wider Chado organism catalog.
- Website Batch Download also offers precomputed files and cross-reference exports. MCP exposes a report-field catalog and paged JSON or TSV. Website reports, mapping results, and vocabulary results have their own layouts and navigation, while MCP exposes structured sections, items and links.
- The coordinate test targets Dmel release 5 to release 6. Run `scripts/coordinate_parity.sh http://127.0.0.1:7200` against a local MCP server for the separate published-anchor check.
- The website RNA-Seq Profile Search uses expression bins and supports combining modENCODE datasets. MCP takes numeric RPKM thresholds against one dataset per call. The website similarity form supports choosing a subset of modENCODE samples. MCP uses all samples in one dataset, includes FlyAtlas2, and computes Pearson correlation on `log2(RPKM + 1)`. The test score checks that MCP calculation from public expression values.
- BLAST testing uses a mock private job API at the MCP boundary and a [small archived release fixture](../blast-jobs/fixture/README.md) at the engine boundary. The [BLAST workflow](../.github/workflows/blast-jobs.yml) runs the engine test with pinned BLAST+ binaries. Routine tests submit jobs only to local fixtures.
- The Wiki tool returns cleaned plain text, canonical links and explicit truncation details. Its tests replay captured API replies for stable coverage of page parsing, search, and redirects.
