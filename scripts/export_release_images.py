#!/usr/bin/env python3
"""Export the data shown on FlyBase image reports from a completed website release."""

import argparse
import json
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path
from zipfile import ZipFile

IMAGE_ID = r"FBim[0-9]{7}"
REFERENCE_ID = r"FBrf[0-9]{7}"
CAPTION_TERM = re.compile(r"@(FBbt:[0-9]+):([^@]+)@")
ANNOTATION_TYPES = {
    "anatomy": ("anatomy", "FBbt"),
    "stage": ("developmental_stage", "FBdv"),
    "GO": ("process", "GO"),
    "image": ("imaging_method", "FBbi"),
}


def required(node, path):
    value = node.findtext(path)
    if value and value.strip():
        return value.strip()
    raise ValueError(f"missing XML field {path}")


def text(node):
    return " ".join("".join(node.itertext()).split())


def plain_text(value):
    # Match FlyBase MCP's notation for superscripts and subscripts in symbols.
    return re.sub(r"</(?:up|down)>", "]", re.sub(r"<(?:up|down)>", "[", value))


def publication_id(pub):
    identifier = pub.findtext("uniquename", "")
    # The report suppresses internal publication placeholders.
    return identifier if re.fullmatch(REFERENCE_ID, identifier) else None


def image_record(raw, identifier, assets):
    image = ET.fromstring(raw).find("image")
    if image is None or required(image, "uniquename") != identifier:
        raise ValueError("image ID does not match archive member")
    props = {}
    for node in image.findall("imageprop"):
        kind = required(node, "type_id/cvterm/name")
        if kind in ("caption", "original", "thumbnail"):
            if kind in props:
                raise ValueError(f"duplicate {kind} property")
            props[kind] = node.find("value")
    for kind, expected in (("original", f"{identifier}.png"), ("thumbnail", f"{identifier}.thumb.png")):
        if text(props[kind]) != expected or expected not in assets:
            raise ValueError(f"invalid or missing {kind} asset {expected}")

    caption_terms = {}

    def caption_label(match):
        term_id, name = match.groups()
        caption_terms[term_id] = name
        return name

    caption = CAPTION_TERM.sub(caption_label, plain_text(text(props["caption"])))
    annotations = {}
    regions = []
    for node in image.findall("image_cvterm"):
        kind = node.findtext("cvterm_type_id/cvterm/name")
        term_path = "cvterm_id/cvterm/"
        dbxref_path = term_path + "dbxref_id/dbxref/"
        prefix = required(node, dbxref_path + "db_id/db/name")
        # The public report displays these four categories of ontology annotation.
        if not kind and prefix in ("FBal", "FBgn", "FBab"):
            continue
        category, expected_prefix = ANNOTATION_TYPES[kind]
        accession = required(node, dbxref_path + "accession")
        if prefix != expected_prefix or not re.fullmatch(r"[0-9]+", accession):
            raise ValueError(f"invalid {kind} accession {prefix}:{accession}")
        term_id = f"{prefix}:{accession}"
        name = required(node, term_path + "name")
        pub = node.find("pub_id/pub")
        reference = publication_id(pub) if pub is not None else None
        annotation = annotations.setdefault(
            (category, term_id),
            {"category": category, "term_id": term_id, "name": name, "has_region": False, "reference_ids": []},
        )
        if name != annotation["name"]:
            raise ValueError(f"inconsistent label for {term_id}")
        if reference and reference not in annotation["reference_ids"]:
            annotation["reference_ids"].append(reference)
        coords = node.findtext("coordinates")
        if coords:
            if kind != "anatomy" or not re.fullmatch(r"[0-9]+,[0-9]+(?:\s+[0-9]+,[0-9]+){2,}", coords):
                raise ValueError(f"invalid image coordinates for {term_id}")
            annotation["has_region"] = True
            regions.append({"term_id": term_id, "coordinates": coords, "reference_id": reference})

    references = []
    for pub in image.findall("image_pub/pub_id/pub"):
        pub_id = publication_id(pub)
        if pub_id:
            references.append({"id": pub_id, "citation": pub.findtext("miniref") or None})
    return {
        "name": plain_text(required(image, "name")),
        "species": " ".join(required(image, f"organism_id/organism/{part}") for part in ("genus", "species")),
        "created": required(image, "timeaccessioned"),
        "updated": required(image, "timelastmodified"),
        "caption": caption,
        "caption_terms": [{"term_id": term_id, "name": name} for term_id, name in caption_terms.items()],
        "annotations": sorted(annotations.values(), key=lambda a: (a["category"], a["name"], a["term_id"])),
        "regions": regions,
        "references": references,
    }


def report_members(archive, suffix):
    members = {}
    for filename in archive.namelist():
        if not re.fullmatch(rf"{IMAGE_ID}\.{suffix}", filename):
            raise ValueError(f"unexpected report archive member {filename}")
        identifier = filename.split(".")[0]
        if identifier in members:
            raise ValueError(f"duplicate report {identifier}")
        members[identifier] = filename
    if not members:
        raise ValueError("empty report archive")
    return members


def export(root, release):
    current = root / "data/downloads/current"
    if not re.fullmatch(r"FB[0-9]{4}_[0-9]{2}", release) or current.readlink().name != release:
        raise ValueError(f"{current} does not identify release {release}")
    data = root / "data"
    with (ZipFile(data / "xml/chado_FBim.zip") as xml,
          ZipFile(data / "xslate/FBim.zip") as rendered,
          ZipFile(data / "images/FBim.zip") as image_files):
        xml_reports = report_members(xml, "xml")
        if xml_reports.keys() != report_members(rendered, "tx").keys():
            raise ValueError("image XML and rendered report archives have different IDs")
        assets = set(image_files.namelist())
        images = {}
        for identifier, member in sorted(xml_reports.items()):
            try:
                images[identifier] = image_record(xml.read(member), identifier, assets)
            except (ValueError, AttributeError, KeyError, ET.ParseError) as error:
                raise ValueError(f"{identifier}: {error}") from error
    return {"release": release, "image_count": len(images), "images": images}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("flybase_server_root", type=Path)
    parser.add_argument("release")
    args = parser.parse_args()
    try:
        projection = export(args.flybase_server_root, args.release)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    json.dump(projection, sys.stdout, ensure_ascii=True, separators=(",", ":"))
    print()


if __name__ == "__main__":
    main()
