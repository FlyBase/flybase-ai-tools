#!/usr/bin/env python3
"""Build the immutable Batch Download index from a verified FlyBase website release."""

import argparse
import contextlib
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import sqlite3
import tempfile
import zlib


# Stable selection keys, release-relative source paths, and eligible record classes.
# The physical interaction, clone and stock entries refer to the files behind the web form.
CATALOG = [
    ("gene_association.fb", "go/gene_association.fb.gz", "FBgn"),
    ("fbgn_annotation_ID", "genes/fbgn_annotation_ID_fb_{v}.tsv.gz", "FBgn"),
    ("fbgn_NAseq_Uniprot", "genes/fbgn_NAseq_Uniprot_fb_{v}.tsv.gz", "FBgn"),
    ("fb_synonym", "synonyms/fb_synonym_fb_{v}.tsv.gz", "FBgn FBal FBab FBba FBtr FBpp FBtp FBms FBmc FBti"),
    ("fbgn_fbtr_fbpp", "genes/fbgn_fbtr_fbpp_fb_{v}.tsv.gz", "FBgn FBtr FBpp"),
    ("fbal_to_fbgn", "alleles/fbal_to_fbgn_fb_{v}.tsv.gz", "FBgn FBal"),
    ("dmel_unique_protein_isoforms", "genes/dmel_unique_protein_isoforms_fb_{v}.tsv.gz", "FBgn"),
    ("dmel_paralogs", "orthologs/dmel_paralogs_fb_{v}.tsv.gz", "FBgn"),
    ("gene_rpkm_report", "genes/gene_rpkm_report_fb_{v}.tsv.gz", "FBgn"),
    ("gene_genetic_interactions", "genes/gene_genetic_interactions_fb_{v}.tsv.gz", "FBgn"),
    ("physical_interactions", "genes/physical_interactions_mitab_fb_{v}.tsv.gz", "FBgn"),
    ("gene_functional_complementation", "genes/gene_functional_complementation_fb_{v}.tsv.gz", "FBgn"),
    ("gene_group_data", "genes/gene_group_data_fb_{v}.tsv.gz", "FBgn FBgg"),
    ("Dmel_enzyme_data", "genes/Dmel_enzyme_data_fb_{v}.tsv.gz", "FBgn FBgg"),
    ("gene_map_table", "genes/gene_map_table_fb_{v}.tsv.gz", "FBgn"),
    ("gene_snapshots", "genes/gene_snapshots_fb_{v}.tsv.gz", "FBgn"),
    ("automated_gene_summaries", "genes/automated_gene_summaries.tsv.gz", "FBgn"),
    ("fbgn_gleanr", "genes/fbgn_gleanr_fb_{v}.tsv.gz", "FBgn"),
    ("fbgn_exons2affy1_overlaps", "genes/fbgn_exons2affy1_overlaps.tsv.gz", "FBgn"),
    ("fbgn_exons2affy2_overlaps", "genes/fbgn_exons2affy2_overlaps.tsv.gz", "FBgn"),
    ("gene_groups_HGNC", "genes/gene_groups_HGNC_fb_{v}.tsv.gz", "FBgg"),
    ("allele_genetic_interactions", "alleles/allele_genetic_interactions_fb_{v}.tsv.gz", "FBal"),
    ("insertion_mapping", "insertions/insertion_mapping_fb_{v}.tsv.gz", "FBti"),
    ("stocks", "stocks/stocks_FB{v}.tsv.gz", "FBst"),
    ("cDNA_clone_data", "reagents/cDNA_clone_data_fb_{v}.tsv.gz", "FBcl"),
    ("genomic_clone_data", "reagents/genomic_clone_data_fb_{v}.tsv.gz", "FBcl"),
    ("dataset_metadata", "metadata/dataset_metadata_fb_{v}.tsv.gz", "FBlc"),
    ("fbrf_pmid_pmcid_doi", "references/fbrf_pmid_pmcid_doi_fb_{v}.tsv.gz", "FBrf"),
]
FBID = re.compile(rb"FB[a-z]{2}[0-9]+")
SEPARATORS = b" \t\n"
MAX_LINE = 1024 * 1024
MAX_BLOCK = 16 * 1024 * 1024
BLOCK_LINES = 256

SCHEMA = """
CREATE TABLE files(id INTEGER PRIMARY KEY, key TEXT UNIQUE NOT NULL, label TEXT NOT NULL,
    path TEXT NOT NULL, classes TEXT NOT NULL, line_count INTEGER NOT NULL, sha256 TEXT NOT NULL);
CREATE TABLE blocks(file INTEGER NOT NULL, block INTEGER NOT NULL, data BLOB NOT NULL,
    PRIMARY KEY(file,block)) WITHOUT ROWID;
CREATE TABLE matches(file INTEGER NOT NULL, id TEXT NOT NULL, block INTEGER NOT NULL,
    PRIMARY KEY(file,id,block)) WITHOUT ROWID;
CREATE TABLE external(kind TEXT NOT NULL, key TEXT NOT NULL, id TEXT NOT NULL,
    PRIMARY KEY(kind,key,id)) WITHOUT ROWID;
"""


def require(condition, message):
    if not condition:
        raise ValueError(message)


def manifest(path, release, **expected):
    value = json.loads(path.read_text())
    for key, item in dict(expected, release=release).items():
        require(value.get(key) == item, f"{path}: expected {key}={item!r}")
    return value


@contextlib.contextmanager
def verified(path, expected_hash, expected_size=None):
    with path.open("rb") as stream:
        before = os.fstat(stream.fileno())
        if expected_size is not None:
            require(before.st_size == expected_size, f"Size mismatch: {path}")
        digest = hashlib.sha256()
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
        require(digest.hexdigest() == expected_hash, f"Checksum mismatch: {path}")
        stream.seek(0)
        yield stream
        after = os.fstat(stream.fileno())
        require((before.st_size, before.st_ctime_ns) == (after.st_size, after.st_ctime_ns),
                f"Input changed during build: {path}")


def bounded_lines(stream):
    while True:
        raw = stream.readline(MAX_LINE + 1)
        if not raw:
            return
        require(len(raw) <= MAX_LINE, "Release data line exceeds the website line limit")
        yield raw


def source_blocks(stream):
    block = bytearray()
    count = 0
    for raw in bounded_lines(stream):
        try:
            raw.decode("utf-8")
        except UnicodeDecodeError:
            # The website decodes each invalid line as Latin-1, leaving its neighbors intact.
            raw = raw.decode("latin-1").encode("utf-8")
        require(len(block) + len(raw) <= MAX_BLOCK, "Release block exceeds the MCP decompression limit")
        block.extend(raw)
        count += 1
        if count == BLOCK_LINES:
            yield count, bytes(block)
            block.clear()
            count = 0
    if count:
        yield count, bytes(block)


def block_ids(raw, physical):
    # Match the website's treatment of CR/LF terminators without changing stored row bytes.
    if b"\r" in raw:
        raw = re.sub(rb"\r+(?=\n|\Z)", b"", raw)
    ids = set()
    if raw.startswith(b"#") or b"\n#" in raw:
        ids.add("")
    for match in FBID.finditer(raw):
        start, end = match.span()
        if (not start or raw[start - 1] in SEPARATORS) and (end == len(raw) or raw[end] in SEPARATORS):
            ids.add(match[0].decode("ascii"))
    if physical:
        for line in raw.split(b"\n"):
            for cell in line.split(b"\t", 2)[:2]:
                if cell.startswith(b"flybase:") and FBID.fullmatch(cell[len(b"flybase:"):]):
                    ids.add(cell[len(b"flybase:"):].decode("ascii"))
    return ids


def add_file(db, file_id, key, relative, classes, root, identities):
    identity = identities[relative]
    count = 0
    require(identity["file"] == relative, f"Unexpected canonical path for {relative}")
    with verified(root / relative, identity["sha256"], identity["bytes"]) as source:
        with gzip.GzipFile(fileobj=source) as stream:
            for block, (size, raw) in enumerate(source_blocks(stream)):
                ids = block_ids(raw, key == "physical_interactions")
                db.execute("INSERT INTO blocks VALUES(?,?,?)", (file_id, block, zlib.compress(raw, 3)))
                db.executemany("INSERT INTO matches VALUES(?,?,?)", ((file_id, i, block) for i in sorted(ids)))
                count += size
    require(count > 0, f"Empty release file: {relative}")
    label = relative.rsplit("/", 1)[-1].removesuffix(".gz")
    db.execute("INSERT INTO files VALUES(?,?,?,?,?,?,?)",
               (file_id, key, label, relative, json.dumps(classes.split()), count, identity["sha256"]))
    print(f"{key}: {count:,} lines", flush=True)


def external_entries(kind, lines):
    dgc = re.compile(r"(FB..\d+)\s+\S+\s+(\S+)\s*")
    accession = re.compile(r"\t(FBgn\d+)[ \t]+(.+)$")
    prefix = re.compile(r"[0-9A-Za-z_]+")
    for raw in lines:
        text = raw.decode("utf-8").rstrip("\n").removesuffix("\r")
        if kind == "dgc":
            match = dgc.fullmatch(text)
            if match:
                yield kind, match[2], match[1]
        elif kind == "accession":
            if not text.startswith("#") and (match := accession.search(text)):
                for token in re.split(r"[ \t]+", match[2]):
                    if re.search(r"\w", token):
                        start = prefix.match(token)
                        yield kind, start[0] if start else token, match[1]
        elif text.strip() and not text.lstrip().startswith("#"):
            fields = text.split("\t")
            require(len(fields) >= 3 and re.fullmatch(r"FBrf[0-9]{7}", fields[0])
                    and re.fullmatch(r"[0-9]*", fields[1]) and re.fullmatch(r"(?:PMC[0-9]+)?", fields[2]),
                    "Invalid reference identifier row")
            for external in fields[:3]:
                if external:
                    yield kind, external, fields[0]


def build(args, temporary):
    release = args.release
    require(re.fullmatch(r"FB[0-9]{4}_[0-9]{2}", release), "Invalid release")
    files = manifest(args.manifest, release, schema=1, kind="batch-download-identities",
                     scope="input-release-tree")["files"]
    aliases = manifest(args.batch / "batch-aliases.json", release, schema="1",
                       scope="full-release", complete="true")
    sources = manifest(args.batch / "batch-external.json", release, schema=1, kind="batch-external",
                       scope="full-release", complete=True)["sources"]
    with verified(args.batch / "batch-aliases.sqlite", aliases["sha256"]) as source, temporary.open("wb") as out:
        shutil.copyfileobj(source, out)
    with contextlib.closing(sqlite3.connect(temporary)) as db:
        meta = dict(db.execute("SELECT key,value FROM metadata"))
        require(all(meta.get(k) == aliases[k] for k in ("release", "complete", "schema", "scope", "rows")),
                "Incompatible website alias index")
        require(db.execute("SELECT count(*) FROM aliases").fetchone()[0] == int(aliases["rows"]),
                "Incomplete website alias index")
        db.execute("PRAGMA journal_mode=OFF")
        db.execute("PRAGMA synchronous=OFF")
        db.execute("PRAGMA cache_size=-65536")
        db.executescript(SCHEMA)
        for file_id, (key, template, classes) in enumerate(CATALOG, 1):
            add_file(db, file_id, key, template.format(v=release[2:]), classes, args.downloads, files)
        for kind, source_key in (("dgc", "dgc"), ("accession", "accessions"), ("publication", "references")):
            identity = sources[source_key]
            filename = identity["file"]
            require(Path(filename).name == filename, "Invalid external source filename")
            with verified(args.batch / filename, identity["sha256"], identity["bytes"]) as source:
                if kind == "dgc":
                    # The website uses the last row for duplicate DGC keys.
                    final = {key: id_ for _, key, id_ in external_entries(kind, bounded_lines(source))}
                    entries = ((kind, key, id_) for key, id_ in final.items())
                    db.executemany("INSERT INTO external VALUES(?,?,?)", entries)
                else:
                    with gzip.GzipFile(fileobj=source) as stream:
                        db.executemany("INSERT OR IGNORE INTO external VALUES(?,?,?)",
                                       external_entries(kind, bounded_lines(stream)))
        db.execute("DELETE FROM metadata")
        db.executemany("INSERT INTO metadata VALUES(?,?)", [("schema", "1"), ("release", release),
                                                            ("complete", "true")])
        db.commit()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--release", required=True)
    parser.add_argument("--downloads", type=Path, required=True, help="Website release downloads directory")
    parser.add_argument("--manifest", type=Path, required=True, help="Website download-manifest.json")
    parser.add_argument("--batch", type=Path, required=True, help="Website release batch lookup directory")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(dir=args.output.parent) as scratch:
        temp = Path(scratch) / "precomputed.sqlite"
        build(args, temp)
        os.chmod(temp, 0o444)
        os.replace(temp, args.output)


if __name__ == "__main__":
    main()
