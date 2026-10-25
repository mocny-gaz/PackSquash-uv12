# Task: build or update the PackSquash baseline jar

You build one jar file that holds the vanilla Minecraft Java Edition assets of many release versions in a compact way. PackSquash reads it with `baseline_pack_path` and `baseline_pack_version`. Follow this document exactly. Do not improvise the output format.

## Inputs you receive

- `build_baseline_jar.py`, attached. It packs version jars into the output format and verifies the result. Do not rewrite it. If it fails, report the error text.
- Optionally, an existing `vanilla-baseline.jar`. If it is attached, run in UPDATE MODE. Otherwise run in BUILD MODE.

## Requirements

- Python 3 and network access to `piston-meta.mojang.com`, `piston-data.mojang.com` (or whatever host the manifest URLs use) and `resources.download.minecraft.net`. If a host is blocked, stop and tell the user which one to allow. Do not invent data.
- Use only URLs that appear in the manifest, version JSON files and asset index. Do not guess URLs.

## Output format (already implemented by the script, listed so you can check it)

```
vanilla-baseline.jar
  baseline.json                 {"format": 1, "versions": ["1.6.1", ...], "removed": {"<id>": ["path", ...]}}
  1.6.1/assets/...              every wanted file of the oldest version
  1.7.2/assets/...              only files added or changed since the previous version
```

- `versions` runs from the oldest to the newest release, in release order. Never sort ids as text.
- A version directory never repeats a file that is unchanged from the previous version.
- `removed` lists files a version deleted relative to the previous version.
- Included: everything under `assets/` except `.ogg` and `.class` files. Add `--include-data` only if the user asks for data pack support.

## Step 1: choose the versions

1. Download `https://piston-meta.mojang.com/mc/game/version_manifest_v2.json`.
2. Keep entries with `"type": "release"`. Drop snapshots, betas and alphas.
3. Sort by `releaseTime`, oldest first.
4. Keep releases from `1.6.1` (the first version with resource packs) up to the newest release.
5. In UPDATE MODE, read `baseline.json` from the existing jar and keep only releases that are newer than its last version and not already in its `versions` list.
6. Write all ids, oldest first, one per line, to `order.txt`. In UPDATE MODE, the file starts with the ids already in the jar, then the new ones.

## Step 2: download each version you still need

Work in a folder `versions/`. For every id that is new (all of them in BUILD MODE):

1. Fetch the `url` of its manifest entry. This is the version JSON.
2. Download `downloads.client.url`. Check its SHA-1 against `downloads.client.sha1`. If it differs, retry once, then stop and report.
3. Save it as `versions/<id>.jar`.
4. Fetch the asset index from `assetIndex.url`. From its `objects` map, take keys that match `^minecraft/(lang/.+|sounds\.json)$`. These files are not in the client jar.
5. For each of those keys, download `https://resources.download.minecraft.net/<first two characters of hash>/<hash>`. Check the SHA-1 equals the hash. Save it to `versions/<id>.extra/assets/<key>`, for example `versions/1.21.4.extra/assets/minecraft/lang/de_de.json`.
6. If an asset index has no keys that match, do not fail. Print the first 20 key names so the user can see what the index holds, and continue.
7. Download politely: one request at a time, retry with a short delay on HTTP 429 or 5xx.

To save disk space, you may replace each downloaded jar by a copy that only keeps entries under `assets/`. The script filters anyway.

## Step 3: run the script

BUILD MODE:

```
python build_baseline_jar.py --versions-dir versions --order order.txt --output vanilla-baseline.jar
```

UPDATE MODE (the existing jar must be at the output path):

```
python build_baseline_jar.py --versions-dir versions --order order.txt --output vanilla-baseline.jar --update
```

The script verifies the result by rebuilding every processed version from the output jar and comparing it with the input. A message that starts with `error:` means the build failed. Do not deliver the jar in that case.

## Step 4: sanity checks before you deliver

Run these and include the results in your report:

1. The `versions` list in `baseline.json` has no duplicates and ends with the newest release.
2. The jar size in MB, and the three versions that added the most data (largest `<id>/` directory in bytes).
3. For the newest version, count the files that resolve (walk from the newest version to the oldest, take the first directory that has the path, stop at a `removed` entry). The count must be larger than zero and in the same range as the file count the script printed for that version.
4. A file that exists in the oldest version and in the newest version with the same content must appear only once in the jar.

## Step 5: deliver

- Deliver `vanilla-baseline.jar` as a file.
- Print the list of versions that are now in it, and in UPDATE MODE the versions you added.
- Print this PackSquash configuration, with the newest version filled in:

```toml
baseline_pack_path = 'path/to/vanilla-baseline.jar'
baseline_pack_version = '<newest id>'
```

## Rules

- Never edit the jar by hand. Never add a version by copying files yourself. The script is the only writer.
- Never reorder or drop versions that are already in the jar.
- If anything fails, stop, report the exact error and which step it happened in. Do not continue with partial data.
- Do not include snapshots or pre-releases, even if the user asks for them in passing. Ask first.

## UPDATE MODE prompt the user can paste later

> Update the PackSquash baseline jar. The current jar and `build_baseline_jar.py` are attached. Follow `baseline-jar-prompt.md` in UPDATE MODE.
