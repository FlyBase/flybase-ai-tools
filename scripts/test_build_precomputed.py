import gzip
import hashlib
import io
import sqlite3
import tempfile
import unittest
import zlib
from pathlib import Path
from unittest.mock import patch

import build_precomputed as build


class ReleaseFileTest(unittest.TestCase):
    def test_index_and_reconstruct_source_blocks(self):
        # IDs in two columns, MITAB participants, exact whitespace, and a CRLF-terminated ID.
        first = b"# Header\nFBgn22\tFBgn22\nXFBgn23 FBgn24.\nflybase:FBgn25\tfoo\tflybase:FBgn26\nFBgn27\r\n"
        raw = first + b"\n" * 251 + "FBgn29\tcafé\n".encode() + b"FBgn31\t\xe9"
        with tempfile.TemporaryDirectory() as folder, sqlite3.connect(":memory:") as db:
            path = Path(folder) / "demo.gz"
            path.write_bytes(gzip.compress(raw))
            proof = {"demo.gz": {"file": "demo.gz", "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                                 "bytes": path.stat().st_size}}
            db.executescript(build.SCHEMA)
            build.add_file(db, 2, "physical_interactions", "demo.gz", "FBgn", path.parent, proof)
            matches = db.execute("SELECT block,id FROM matches ORDER BY block,id").fetchall()
            self.assertEqual(matches, [(0, ""), (0, "FBgn22"), (0, "FBgn25"), (0, "FBgn27"), (1, "FBgn29"), (1, "FBgn31")])
            self.assertEqual(db.execute("SELECT line_count FROM files").fetchone(), (258,))
            text = b"".join(zlib.decompress(row[0]) for row in db.execute("SELECT data FROM blocks ORDER BY block"))
            self.assertEqual(text, raw[:-1] + "é".encode())
            proof["demo.gz"]["sha256"] = "0" * 64
            with self.assertRaisesRegex(ValueError, "Checksum mismatch"):
                build.add_file(db, 3, "physical_interactions", "demo.gz", "FBgn", path.parent, proof)

    def test_limits_apply_before_publication(self):
        with patch.object(build, "MAX_BLOCK", 5):
            with self.assertRaisesRegex(ValueError, "decompression limit"):
                list(build.source_blocks(io.BytesIO(b"aaa\nbb\n")))
        with patch.object(build, "MAX_LINE", 5):
            with self.assertRaisesRegex(ValueError, "line limit"):
                list(build.source_blocks(io.BytesIO(b"abcdef\n")))

    def test_external_lookup_keys_match_website_inputs(self):
        self.assertEqual(list(build.external_entries("dgc", [b"FBgn123 label DGCA1\n"])),
                         [("dgc", "DGCA1", "FBgn123")])
        self.assertEqual(list(build.external_entries("accession", [b"Dmel\tFBgn123\tNP_123.2 Q12\n"])),
                         [("accession", "NP_123", "FBgn123"), ("accession", "Q12", "FBgn123")])
        ids = list(build.external_entries("publication", [b"FBrf0000001\t12\tPMC12\ta.doi\n"]))
        self.assertEqual({value for _, value, _ in ids}, {"12", "PMC12", "FBrf0000001"})


if __name__ == "__main__":
    unittest.main()
