# Fixture database

A small Postgres 16 with the public chado schema and rows for the IDs in `ids.txt` plus their neighbours.

    fixture/export.sh   # needs psql and pg_dump 16; reads public chado; writes data/ (git-ignored); a few minutes
    fixture/load.sh     # docker compose up + \copy; prints each table loaded
    psql postgres://flybase_mcp:flybase_mcp@127.0.0.1:5499/flybase -f fixture/smoke.sql

Tests connect to `CHADO_URL`, defaulting to the URL above. Re-run both scripts after changing `ids.txt` or `export.sh`.

Feature rows cover the IDs in `ids.txt`, every feature one relationship away from them, the seeds' source features,
the children (exons, polypeptides) of seed genes' transcripts, and all Dmel chromosome bands. Neighbours of neighbours
are not included: the plan's generic second hop reached 208k features because Notch alone has 2,305 direct neighbours.
Three narrower additions serve the Plan 2 tests: the physical-interaction partner hop (the `-XP` polypeptides of seed
genes, their `feature_interaction` partners, and the partners' genes), the polypeptide domains located on seed
polypeptides, and the first 5,000 clones of library `FBlc0000001` (the `library_feature` sample, 5,000 rows).
`library_relationship` is copied whole; besides the 5,000-row `FBlc0000001` sample, `library_feature` and
`library_featureprop` carry the RNA-seq RPKM rows for the fixture features only, so migration 005 builds a matrix of a
few hundred genes.
`dbxref` holds only the rows referenced by exported feature, feature_dbxref, pub_dbxref and cvterm rows, so
`humanhealth_dbxref` rows have no `dbxref` row here and that field is null on the fixture.
Public chado drops connections on queries that run past about two minutes, so `export.sh` keeps every copy small.

`cell_line`, `strain` and `library` with their synonym tables are copied whole, and `synonym` holds the rows those and the
feature, gene group and human health synonym tables reference. Golden paths are copied whole for Dmel X, 2L and 3R only,
plus the unsequenced Dpse scaffold `3_groupMISC`; Dmel 2R, 3L, 4 and Y are present with NULL residues and their true
`seqlen` so `mcp.band_coordinates` covers every arm (`get_sequence` on them reports no stored sequence).

Plan 2 tables: `feature_grpmember`, `grp_relationship`, `grp_pub`, `grp_cvterm`, the `humanhealth_*` side tables,
`cell_lineprop`, `cell_line_pub`, `cell_line_cvterm`, `cell_line_feature`, `strainprop`, `strain_pub`, `libraryprop`,
`library_pub`, `library_cvterm`, `cvtermprop`, `phenotype_comparison_cvterm`, `dataclass_relationship.gene_diseasev2`,
`humanhealth.allele_disease_variant`, `gene.split_system_combination(_component_allele)` and `gene_group.pathway_member`
are copied whole. `feature_relationship_pub`, `feature_relationshipprop`, `feature_interaction`, `interaction`,
`interaction_pub`, `interactionprop`, `interaction_cvterm` and `phenotype_comparison` are restricted to the feature-id set.

`fixture/data/diopt.tsv.gz` (git-ignored, downloaded by hand) is optional; no test depends on it. When it is
absent, `tests/migrations.rs::diopt_loader_parses_the_release_file_format` leaves `mcp.diopt_orthologs` empty
(truncated) after loading its sample file. That is harmless on the fixture, but running the suite against your own
restore empties a DIOPT table you loaded there, so reload it with `--diopt` afterwards.

Known gaps: `stock_pub` is empty in public chado.
