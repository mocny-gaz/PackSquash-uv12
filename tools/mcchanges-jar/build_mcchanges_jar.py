#!/usr/bin/env python3
"""Builds the mcchanges jar that PackSquash reads with mcchanges_path and mcchanges_version.

An mcchanges jar holds the vanilla files of many Minecraft versions in a compact way.
Every version is compared with the version BEFORE it, and only the files that the
version added or changed are stored, plus a list of the files that it deleted.

Commands (Python 3, no extra packages):

  Java Edition, downloads the releases from Mojang:
    python build_mcchanges_jar.py java --output mcchanges-java.jar
    python build_mcchanges_jar.py java --output mcchanges-java.jar --update

  Bedrock Edition, downloads the Mojang/bedrock-samples releases from GitHub:
    python build_mcchanges_jar.py bedrock --output mcchanges-bedrock.jar
    python build_mcchanges_jar.py bedrock --output mcchanges-bedrock.jar --update

  Offline, from files that you already have (DIR/<id>.jar and an order file):
    python build_mcchanges_jar.py pack --edition java --versions-dir DIR --order order.txt --output out.jar

  Add --dry-run to any download command to see what it would do.

--update keeps the versions that are already in the output file and appends the
releases that came out after them. The result is identical to a full rebuild.

Output layout:
  mcchanges.json            {"format": 1, "edition": "...", "versions": [...], "removed": {...}}
  <oldest id>/...           every file of the oldest version
  <next id>/...             only files added or changed since the version before it
  "removed" lists, for each version, the files it deleted compared with the version before.

The files inside belong to Mojang. Build the jar on your own computer and do not publish it.
"""

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import zipfile

INDEX_NAME = "mcchanges.json"
FORMAT = 1
FIXED_DATE = (1980, 1, 1, 0, 0, 0)  # makes the output reproducible
USER_AGENT = "mcchanges-jar-builder (PackSquash fork)"

JAVA_MANIFEST_URL = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
JAVA_RESOURCES_URL = "https://resources.download.minecraft.net"
# Files that are not in the client jar, but are in the asset index
JAVA_EXTRA_KEY = re.compile(r"^minecraft/(lang/.+|sounds\.json)$")

BEDROCK_REPO = "Mojang/bedrock-samples"
BEDROCK_TAG = re.compile(r"^v(\d+(?:\.\d+)+)(-preview)?$")


def log(message):
    print(message, flush=True)


def fail(message):
    sys.exit(f"error: {message}")


# ----------------------------------------------------------------------------
# Which files go in the jar
# ----------------------------------------------------------------------------

class Filter:
    def __init__(self, edition, include_data=False, include_audio=False):
        self.edition = edition
        self.include_data = include_data
        self.include_audio = include_audio

    def wanted(self, path):
        if not path or path.endswith("/") or path.endswith(".class"):
            return False
        if not self.include_audio and path.endswith((".ogg", ".fsb")):
            return False
        if self.edition == "java":
            return path.startswith("assets/") or (self.include_data and path.startswith("data/"))
        return True


# ----------------------------------------------------------------------------
# Network
# ----------------------------------------------------------------------------

def open_url(url, token=None, retries=4):
    headers = {"User-Agent": USER_AGENT}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    for attempt in range(retries):
        try:
            return urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=60)
        except urllib.error.HTTPError as err:
            if err.code not in (429, 500, 502, 503, 504) or attempt == retries - 1:
                fail(f"{url} answered HTTP {err.code}")
        except (urllib.error.URLError, TimeoutError, ConnectionError) as err:
            if attempt == retries - 1:
                fail(f"could not reach {url}: {err}")
        time.sleep(2 * (attempt + 1))


def fetch_json(url, token=None):
    with open_url(url, token) as response:
        return json.load(response)


def download(url, destination, sha1=None, token=None):
    """Downloads a file, checking its SHA-1 if it is known, and replaces the destination."""
    part = destination + ".part"
    for attempt in range(2):
        digest = hashlib.sha1()
        with open_url(url, token) as response, open(part, "wb") as out:
            while chunk := response.read(1 << 20):
                digest.update(chunk)
                out.write(chunk)
        if sha1 is None or digest.hexdigest() == sha1:
            os.replace(part, destination)
            return
        log(f"  SHA-1 mismatch for {url}, retrying")
    os.unlink(part)
    fail(f"the SHA-1 of {url} does not match after a retry")


# ----------------------------------------------------------------------------
# Reading and writing jars
# ----------------------------------------------------------------------------

def sha256(data):
    return hashlib.sha256(data).hexdigest()


def read_zip(path, flt, prefix=""):
    """Returns {path: bytes} for the wanted files of a ZIP file, without the prefix."""
    files = {}
    with zipfile.ZipFile(path) as archive:
        for info in archive.infolist():
            name = info.filename
            # Directories are skipped, including the one that the prefix names
            if not name.endswith("/") and name.startswith(prefix) and flt.wanted(name[len(prefix):]):
                files[name[len(prefix):]] = archive.read(info)
    return files


def write_zip(destination, entries):
    """Writes {name: bytes} sorted by name through a temporary file."""
    directory = os.path.dirname(os.path.abspath(destination))
    handle, temporary = tempfile.mkstemp(suffix=".tmp", dir=directory)
    os.close(handle)
    try:
        with zipfile.ZipFile(temporary, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for name in sorted(entries):
                info = zipfile.ZipInfo(name, FIXED_DATE)
                info.compress_type = zipfile.ZIP_DEFLATED
                info.external_attr = 0o644 << 16
                archive.writestr(info, entries[name], compresslevel=9)
        os.replace(temporary, destination)
    except BaseException:
        if os.path.exists(temporary):
            os.unlink(temporary)
        raise


def load_version(versions_dir, version, flt):
    """Returns {path: bytes} of a version from DIR/<id>.jar and the optional DIR/<id>.extra."""
    jar_path = os.path.join(versions_dir, version + ".jar")
    if not os.path.isfile(jar_path):
        fail(f"missing {jar_path}")
    files = read_zip(jar_path, flt)

    extra_dir = os.path.join(versions_dir, version + ".extra")
    for root, _, names in os.walk(extra_dir):
        for name in names:
            full = os.path.join(root, name)
            path = os.path.relpath(full, extra_dir).replace(os.sep, "/")
            if flt.wanted(path):
                with open(full, "rb") as f:
                    files[path] = f.read()
    return files


def read_existing(path):
    """Returns (index, effective hashes of the newest version, all entries)."""
    with zipfile.ZipFile(path) as archive:
        if INDEX_NAME not in archive.namelist():
            fail(f"{path} has no {INDEX_NAME}, so it is not an mcchanges jar")
        index = json.loads(archive.read(INDEX_NAME))
        if index.get("format") != FORMAT:
            fail(f"{path} has an unsupported format")
        entries = {i.filename: archive.read(i) for i in archive.infolist() if not i.filename.endswith("/")}

    effective = {}
    for version in index["versions"]:
        for removed in index.get("removed", {}).get(version, []):
            effective.pop(removed, None)
        prefix = version + "/"
        for name, data in entries.items():
            if name.startswith(prefix):
                effective[name[len(prefix):]] = sha256(data)
    return index, effective, entries


def verify(output, expected):
    """Rebuilds each processed version from the output file and compares it with its input."""
    with zipfile.ZipFile(output) as archive:
        index = json.loads(archive.read(INDEX_NAME))
        names = [i.filename for i in archive.infolist() if not i.filename.endswith("/")]
        effective = {}
        for version in index["versions"]:
            for removed in index.get("removed", {}).get(version, []):
                effective.pop(removed, None)
            prefix = version + "/"
            for name in names:
                if name.startswith(prefix):
                    effective[name[len(prefix):]] = sha256(archive.read(name))
            if version in expected and effective != expected[version]:
                want = expected[version]
                fail(
                    f"verification failed for {version}: {len(set(want) - set(effective))} missing, "
                    f"{len(set(effective) - set(want))} extra, "
                    f"{sum(1 for p in effective if p in want and effective[p] != want[p])} different"
                )


def pack(edition, flt, versions_dir, order, output, update):
    """Packs the versions of `order` that are not in the output yet. Returns nothing."""
    if len(order) != len(set(order)):
        fail("duplicate version ids")
    if any("/" in v or "\\" in v or v in (".", "..") for v in order):
        fail("invalid version id")

    if update and os.path.exists(output):
        index, previous, entries = read_existing(output)
        existing = index["versions"]
        if order[: len(existing)] != existing:
            fail("the versions already in the output are not the start of the version list")
        removed_map = index.get("removed", {})
    else:
        existing, previous, entries, removed_map = [], {}, {}, {}

    new_versions = order[len(existing):]
    if not new_versions:
        log("nothing to add")
        return

    expected = {}
    for version in new_versions:
        files = load_version(versions_dir, version, flt)
        if not files:
            fail(f"no files found for {version}")
        hashes = {path: sha256(data) for path, data in files.items()}
        expected[version] = hashes

        # Each version is compared with the version right before it
        changed = [p for p in files if previous.get(p) != hashes[p]]
        for path in changed:
            entries[f"{version}/{path}"] = files[path]
        removed = sorted(p for p in previous if p not in files)
        if removed:
            removed_map[version] = removed
        log(f"{version}: {len(changed)} added or changed, {len(removed)} removed, {len(files)} total")
        previous = hashes

    index = {"format": FORMAT, "edition": edition, "versions": existing + new_versions}
    if removed_map:
        index["removed"] = removed_map
    entries[INDEX_NAME] = json.dumps(index, indent=1, sort_keys=True).encode("utf-8")

    write_zip(output, entries)
    verify(output, expected)
    log(f"wrote {output} ({os.path.getsize(output) / 1e6:.1f} MB), verified {len(new_versions)} version(s)")


def existing_versions(output, update):
    if update and os.path.exists(output):
        return read_existing(output)[0]["versions"]
    return []


# ----------------------------------------------------------------------------
# Java Edition
# ----------------------------------------------------------------------------

def java_releases(manifest_url):
    manifest = fetch_json(manifest_url)
    releases = [v for v in manifest["versions"] if v.get("type") == "release"]
    releases.sort(key=lambda v: v["releaseTime"])
    return releases


def fetch_java_version(entry, work_dir, flt, resources_url):
    version = entry["id"]
    ready = os.path.join(work_dir, version + ".ready")
    if os.path.exists(ready):
        return
    log(f"{version}: downloading")

    data = fetch_json(entry["url"])
    client = data["downloads"]["client"]
    raw = os.path.join(work_dir, version + ".download")
    download(client["url"], raw, client.get("sha1"))
    # Keep only what is needed, to save disk space
    write_zip(os.path.join(work_dir, version + ".jar"), read_zip(raw, flt))
    os.unlink(raw)

    # Language files and sounds.json are not in the client jar, but in the asset index
    index = fetch_json(data["assetIndex"]["url"])
    objects_dir = os.path.join(work_dir, "objects")
    os.makedirs(objects_dir, exist_ok=True)
    extra_dir = os.path.join(work_dir, version + ".extra")
    found = 0
    for key, obj in sorted(index.get("objects", {}).items()):
        if not JAVA_EXTRA_KEY.match(key) or not flt.wanted("assets/" + key):
            continue
        cached = os.path.join(objects_dir, obj["hash"])
        if not os.path.exists(cached):
            download(f"{resources_url}/{obj['hash'][:2]}/{obj['hash']}", cached, obj["hash"])
        target = os.path.join(extra_dir, "assets", *key.split("/"))
        os.makedirs(os.path.dirname(target), exist_ok=True)
        shutil.copyfile(cached, target)
        found += 1
    if not found:
        sample = ", ".join(sorted(index.get("objects", {}))[:8])
        log(f"  note: no language files in the asset index of {version}. It has keys like: {sample}")

    open(ready, "w").close()


def cmd_java(args):
    flt = Filter("java", args.include_data, args.include_audio)
    releases = java_releases(args.manifest_url)
    ids = [r["id"] for r in releases]

    if args.last:
        selected = releases[-args.last:]
    else:
        if args.first not in ids:
            fail(f"the release {args.first} was not found")
        selected = releases[ids.index(args.first):]

    existing = existing_versions(args.output, args.update)
    if existing:
        if existing[-1] not in ids:
            fail(f"{existing[-1]} is not a release in the Mojang version list")
        if existing != [i for i in ids if i in set(existing)]:
            fail("the versions in the output file are not in release order")
        # Continue after the last version that the file has
        selected = releases[ids.index(existing[-1]) + 1:]
    new = selected
    order = existing + [r["id"] for r in new]

    log(f"{len(existing)} version(s) in the file, {len(new)} to add: {', '.join(r['id'] for r in new) or '-'}")
    if args.dry_run or not new:
        return

    os.makedirs(args.work_dir, exist_ok=True)
    for entry in new:
        fetch_java_version(entry, args.work_dir, flt, args.resources_url)
    pack("java", flt, args.work_dir, order, args.output, args.update)
    if args.clean_work:
        shutil.rmtree(args.work_dir, ignore_errors=True)


# ----------------------------------------------------------------------------
# Bedrock Edition
# ----------------------------------------------------------------------------

def bedrock_tag_names(api_url, token):
    """Lists the tags of the repository. Git is tried first, as it has no rate limits."""
    if shutil.which("git"):
        result = subprocess.run(
            ["git", "ls-remote", "--tags", "--refs", f"https://github.com/{BEDROCK_REPO}.git"],
            capture_output=True, text=True
        )
        if result.returncode == 0 and result.stdout.strip():
            return [line.split("refs/tags/", 1)[1] for line in result.stdout.splitlines() if "refs/tags/" in line]

    tags, page = [], 1
    while True:
        batch = fetch_json(f"{api_url}/repos/{BEDROCK_REPO}/tags?per_page=100&page={page}", token)
        tags += [t["name"] for t in batch]
        if len(batch) < 100:
            return tags
        page += 1


def bedrock_releases(api_url, include_previews, token):
    """Returns [(version id, tag)] from the oldest to the newest."""
    found = []
    for tag in bedrock_tag_names(api_url, token):
        match = BEDROCK_TAG.match(tag)
        if match and (include_previews or not match.group(2)):
            key = tuple(int(part) for part in match.group(1).split("."))
            found.append((key, tag[1:], tag))
    found.sort()
    return [(version, tag) for _, version, tag in found]


def fetch_bedrock_version(version, tag, work_dir, flt, token):
    ready = os.path.join(work_dir, version + ".ready")
    if os.path.exists(ready):
        return
    log(f"{version}: downloading (this file is big, around 150 MB)")

    raw = os.path.join(work_dir, version + ".download")
    download(f"https://codeload.github.com/{BEDROCK_REPO}/zip/refs/tags/{tag}", raw, None, token)

    with zipfile.ZipFile(raw) as archive:
        roots = {n.split("/")[0] for n in archive.namelist()}
    if len(roots) != 1:
        fail(f"unexpected layout in the download of {tag}")
    prefix = roots.pop() + "/resource_pack/"
    files = read_zip(raw, flt, prefix)
    if not files:
        fail(f"{tag} has no resource_pack folder")
    write_zip(os.path.join(work_dir, version + ".jar"), files)
    os.unlink(raw)
    open(ready, "w").close()


def cmd_bedrock(args):
    flt = Filter("bedrock", False, args.include_audio)
    token = args.github_token or os.environ.get("GITHUB_TOKEN")
    releases = bedrock_releases(args.api_url, args.include_previews, token)
    ids = [v for v, _ in releases]
    if not ids:
        fail("no releases were found")

    existing = existing_versions(args.output, args.update)
    if existing:
        if existing[-1] not in ids:
            fail(f"{existing[-1]} is not in the release list. Use --include-previews if it was a preview")
        start = ids.index(existing[-1]) + 1
    elif args.first:
        if args.first not in ids:
            fail(f"{args.first} was not found. Releases: {', '.join(ids[-10:])}...")
        start = ids.index(args.first)
    else:
        start = max(0, len(ids) - args.count)
    new = releases[start:]
    order = existing + [v for v, _ in new]

    log(f"{len(existing)} version(s) in the file, {len(new)} to add: {', '.join(v for v, _ in new) or '-'}")
    if args.dry_run or not new:
        return

    os.makedirs(args.work_dir, exist_ok=True)
    for version, tag in new:
        fetch_bedrock_version(version, tag, args.work_dir, flt, token)
    pack("bedrock", flt, args.work_dir, order, args.output, args.update)
    if args.clean_work:
        shutil.rmtree(args.work_dir, ignore_errors=True)


# ----------------------------------------------------------------------------
# Offline packing
# ----------------------------------------------------------------------------

def cmd_pack(args):
    flt = Filter(args.edition, args.include_data, args.include_audio)
    with open(args.order, encoding="utf-8") as f:
        order = [line.strip() for line in f if line.strip() and not line.startswith("#")]
    pack(args.edition, flt, args.versions_dir, order, args.output, args.update)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    def common(p):
        p.add_argument("--output", required=True, help="the mcchanges jar to write")
        p.add_argument("--update", action="store_true", help="append new versions to the existing output")
        p.add_argument("--include-audio", action="store_true", help="also include .ogg and .fsb files (big)")

    def downloading(p):
        common(p)
        p.add_argument("--work-dir", default="mcchanges-work", help="where downloads are kept (default: %(default)s)")
        p.add_argument("--clean-work", action="store_true", help="delete the work directory when done")
        p.add_argument("--dry-run", action="store_true", help="only show what would be done")

    java = sub.add_parser("java", help="build from the Mojang Java Edition releases")
    downloading(java)
    java.add_argument("--first", default="1.6.1", help="oldest release to include (default: %(default)s)")
    java.add_argument("--last", type=int, help="only the newest N releases, instead of --first")
    java.add_argument("--include-data", action="store_true", help="also include data/ (data packs)")
    java.add_argument("--manifest-url", default=JAVA_MANIFEST_URL, help=argparse.SUPPRESS)
    java.add_argument("--resources-url", default=JAVA_RESOURCES_URL, help=argparse.SUPPRESS)
    java.set_defaults(run=cmd_java)

    bedrock = sub.add_parser("bedrock", help="build from the Mojang/bedrock-samples releases")
    downloading(bedrock)
    bedrock.add_argument("--count", type=int, default=5, help="how many of the newest releases (default: %(default)s)")
    bedrock.add_argument("--first", help="oldest release to include, for example 1.21.50.7, instead of --count")
    bedrock.add_argument("--include-previews", action="store_true", help="also include preview releases")
    bedrock.add_argument("--github-token", help="GitHub token (or set GITHUB_TOKEN), raises the API rate limit")
    bedrock.add_argument("--api-url", default="https://api.github.com", help=argparse.SUPPRESS)
    bedrock.set_defaults(run=cmd_bedrock)

    packing = sub.add_parser("pack", help="build from version files that you already have")
    common(packing)
    packing.add_argument("--edition", choices=["java", "bedrock"], required=True)
    packing.add_argument("--versions-dir", required=True, help="has <id>.jar and optional <id>.extra/ per version")
    packing.add_argument("--order", required=True, help="text file with one version id per line, oldest first")
    packing.add_argument("--include-data", action="store_true")
    packing.set_defaults(run=cmd_pack)

    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
