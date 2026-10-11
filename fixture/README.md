# Fixture database

A small Postgres 16 with the public chado schema and rows for the IDs in `ids.txt` plus their neighbours.

    fixture/export.sh   # needs psql and pg_dump 16; reads public chado; writes data/ (git-ignored); a few minutes
    fixture/load.sh     # docker compose up + \copy; prints each table loaded
    psql postgres://flybase_mcp:flybase_mcp@127.0.0.1:5499/flybase -f fixture/smoke.sql

Tests connect to `CHADO_URL`, defaulting to the URL above. Re-run both scripts after changing `ids.txt` or `export.sh`.

Feature rows cover the IDs in `ids.txt`, every feature one relationship away from them, the seeds' source features,
the children (exons, polypeptides) of seed genes' transcripts, and all Dmel chromosome bands. Neighbours of neighbours
are not included: the plan's generic second hop reached 208k features because Notch alone has 2,305 direct neighbours.
`dbxref` holds only the rows referenced by exported feature, feature_dbxref, pub_dbxref and cvterm rows.
Public chado drops connections on queries that run past about two minutes, so `export.sh` keeps every copy small.

Known gaps: `stock_pub` is empty in public chado; `library_*` and `interaction` tables are not exported (Plans 2 and 3 extend this).
