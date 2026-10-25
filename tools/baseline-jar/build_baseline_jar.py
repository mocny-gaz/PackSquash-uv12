#!/usr/bin/env python3
"""Packs the assets of many Minecraft versions into one baseline jar for PackSquash.

Usage:
  build_baseline_jar.py --versions-dir DIR --order versions.txt --output baseline.jar
  build_baseline_jar.py --versions-dir DIR --order versions.txt --output baseline.jar --update

Inputs:
  versions.txt   One version id per line, from the OLDEST to the NEWEST release.
  DIR/<id>.jar   The client jar of each version in versions.txt.
  DIR/<id>.extra/ Optional. Files that are not in the client jar, such as the language
                 files of the asset index. Same layout as the jar, for example
                 DIR/1.21.4.extra/assets/minecraft/lang/de_de.json. They take
                 precedence over the jar files with the same path.

Output layout (what PackSquash reads with baseline_pack_path and baseline_pack_version):
  baseline.json         {"format": 1, "versions": [...], "removed": {"<id>": [paths]}}
  <oldest id>/assets/...  every file of the oldest version
  <other id>/assets/...   only files that the version added or changed
  "removed" lists the files each version deleted relative to the previous one.

--update keeps the versions already in the output file and only appends the ids of
versions.txt that are not in it. The versions already in the file must be a prefix of
versions.txt, in the same order.

Only the Python standard library is used.
"""

import argparse
import hashlib
import json
import os
import sys
import tempfile
import zipfile

INDEX_NAME = "baseline.json"
EXCLUDED_SUFFIXES = (".ogg", ".class")
# A fixed timestamp makes the output reproducible
FIXED_DATE = (1980, 1, 1, 0, 0, 0)


def is_wanted(path, include_data):
    if path.endswith("/") or path.endswith(EXCLUDED_SUFFIXES):
        return False
    return path.startswith("assets/") or (include_data and path.startswith("data/"))


def load_version(versions_dir, version, include_data):
    """Returns {path: bytes} for the wanted files of a version."""
    jar_path = os.path.join(versions_dir, version + ".jar")
    if not os.path.isfile(jar_path):
        sys.exit(f"error: missing {jar_path}")

    files = {}
    with zipfile.ZipFile(jar_path) as jar:
        for info in jar.infolist():
            if is_wanted(info.filename, include_data):
                files[info.filename] = jar.read(info)

    extra_dir = os.path.join(versions_dir, version + ".extra")
    if os.path.isdir(extra_dir):
        for root, _, names in os.walk(extra_dir):
            for name in names:
                full = os.path.join(root, name)
                path = os.path.relpath(full, extra_dir).replace(os.sep, "/")
                if is_wanted(path, include_data):
                    with open(full, "rb") as f:
                        files[path] = f.read()
    return files


def digest(data):
    return hashlib.sha256(data).hexdigest()


def read_existing(path):
    """Returns (index, effective hashes of the newest version, kept entries)."""
    with zipfile.ZipFile(path) as jar:
        index = json.loads(jar.read(INDEX_NAME))
        if index.get("format") != 1:
            sys.exit(f"error: {path} has an unsupported format")
        entries = {i.filename: jar.read(i) for i in jar.infolist() if not i.filename.endswith("/")}

    effective = {}
    for version in index["versions"]:
        for removed in index.get("removed", {}).get(version, []):
            effective.pop(removed, None)
        prefix = version + "/"
        for name, data in entries.items():
            if name.startswith(prefix):
                effective[name[len(prefix):]] = digest(data)
    return index, effective, entries


def write_jar(output, entries):
    """Writes {name: bytes} sorted by name, to a temporary file first."""
    directory = os.path.dirname(os.path.abspath(output))
    handle, temporary = tempfile.mkstemp(suffix=".tmp", dir=directory)
    os.close(handle)
    try:
        with zipfile.ZipFile(temporary, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as jar:
            for name in sorted(entries):
                info = zipfile.ZipInfo(name, FIXED_DATE)
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o644 << 16
                jar.writestr(info, entries[name], compresslevel=9)
        os.replace(temporary, output)
    except BaseException:
        os.unlink(temporary)
        raise


def verify(output, expected_hashes):
    """Rebuilds each processed version from the output and compares it with the input."""
    with zipfile.ZipFile(output) as jar:
        index = json.loads(jar.read(INDEX_NAME))
        names = [i.filename for i in jar.infolist() if not i.filename.endswith("/")]
        effective = {}
        for version in index["versions"]:
            for removed in index.get("removed", {}).get(version, []):
                effective.pop(removed, None)
            prefix = version + "/"
            for name in names:
                if name.startswith(prefix):
                    effective[name[len(prefix):]] = digest(jar.read(name))
            if version in expected_hashes and effective != expected_hashes[version]:
                missing = set(expected_hashes[version]) - set(effective)
                extra = set(effective) - set(expected_hashes[version])
                sys.exit(
                    f"error: verification failed for {version}: {len(missing)} missing, "
                    f"{len(extra)} extra, "
                    f"{sum(1 for p in effective if p in expected_hashes[version] and effective[p] != expected_hashes[version][p])} different"
                )


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--versions-dir", required=True)
    parser.add_argument("--order", required=True, help="text file with one version id per line, oldest first")
    parser.add_argument("--output", required=True)
    parser.add_argument("--update", action="store_true", help="append new versions to the existing output")
    parser.add_argument("--include-data", action="store_true", help="also include data/ (data packs)")
    args = parser.parse_args()

    with open(args.order, encoding="utf-8") as f:
        order = [line.strip() for line in f if line.strip() and not line.startswith("#")]
    if len(order) != len(set(order)):
        sys.exit("error: duplicate version ids in the order file")
    if any("/" in v or "\\" in v or v in (".", "..") for v in order):
        sys.exit("error: invalid version id")

    if args.update:
        index, previous, entries = read_existing(args.output)
        existing = index["versions"]
        if order[: len(existing)] != existing:
            sys.exit("error: the versions already in the output are not a prefix of the order file")
        removed_map = index.get("removed", {})
    else:
        existing, previous, entries, removed_map = [], {}, {}, {}

    new_versions = order[len(existing):]
    if not new_versions:
        print("nothing to add")
        return

    expected_hashes = {}
    for version in new_versions:
        files = load_version(args.versions_dir, version, args.include_data)
        if not files:
            sys.exit(f"error: no assets found for {version}")
        hashes = {path: digest(data) for path, data in files.items()}
        expected_hashes[version] = hashes

        changed = [p for p in files if previous.get(p) != hashes[p]]
        for path in changed:
            entries[f"{version}/{path}"] = files[path]
        removed = sorted(p for p in previous if p not in files)
        if removed:
            removed_map[version] = removed
        print(f"{version}: {len(changed)} added or changed, {len(removed)} removed, {len(files)} total")
        previous = hashes

    index = {"format": 1, "versions": order[: len(existing)] + new_versions}
    if removed_map:
        index["removed"] = removed_map
    entries[INDEX_NAME] = json.dumps(index, indent=1, sort_keys=True).encode("utf-8")

    write_jar(args.output, entries)
    verify(args.output, expected_hashes)
    print(f"wrote {args.output} ({os.path.getsize(args.output)} bytes), verified {len(new_versions)} version(s)")


if __name__ == "__main__":
    main()
