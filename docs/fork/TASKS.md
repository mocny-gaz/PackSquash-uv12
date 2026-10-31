# Task list of the fork

Updated 2026-10-31. Edit it freely and upload it. One line per task: `- [ ] ID text (who)`. Put long text, like an error, in a
block under the task. Fixed things move to Done, in one short line.

## This very moment

- [ ] t2 `release.py` said the program imports `VCRUNTIME140.dll`, and rustc warned about `generic_const_exprs`. Both fixed, owner checks (patch 0016, new `release.py`: it now builds with the GNU toolchain like the CI, run `python release.py setup` once) (owner)
- [ ] n8 `--include-previews` crash `File is not a zip file`. A damaged saved download is now found and downloaded again, and the error names the version. Could not reproduce it from a clean folder, so run it again; if it still fails send the first lines (owner)
- [ ] N7 Dependabot (1 critical, 1 high, 10 moderate, 3 low): `tokio`, `libz-sys`, `ctutils`, `certifi` (patch 0013) and `filelock` (13 alerts, patch 0015). Apply them, push, check the page (owner)

## Now

- [ ] N1 Apply patches 0011, 0012, 0013, 0014, 0015, 0016 with `apply_patches.py` (0010 is already in master) (owner)
- [ ] N6 Run nightly `cargo fmt --all` once, commit (owner)
- [ ] N4 First release: `python release.py setup`, then `python release.py release --dry-run`, then `python release.py release` (owner)
- [ ] N5 Publish the jars in the `mc-changes` repository as GitHub Release files, put README.md and PUBLISHING.md in it. Rebuild the jars with the new `build_mcchanges_jar.py` first (owner)

## Features (the AI starts after the first release)

- [ ] F1 Any archive format as input (`.7z`, `.tar`, `.tar.gz`, `.tar.xz`, `.tar.zst`, `.rar` read only, others), found by looking at the contents: `pack.mcmeta` with `assets/`, or a Bedrock `manifest.json`, also inside a subfolder. The output stays a zip (AI)
- [ ] F2 Bulk squash: every pack found in a directory, whatever it looks like. Output next to the input with the prefix `squashed_` by default; options for a custom prefix and suffix, an overwrite option (keep a backup unless told not to), a dry run and a report (AI)
- [ ] F3 Very simple GUI like Win11Debloat, only dark OLED mode, easy settings, export and import of the settings. Later release, last of all (AI)

## Later

- [ ] L1 Issue 119: shader minification. Needs study
- [ ] L2 Issue 362: function inlining. Needs study
- [ ] L3 Issue 300: Nemo's Transit Expansion. Needs study
- [ ] L4 Upstream issues 254 (overlays) and 373 (formats): wait for upstream
- [ ] L5 Not planned: 196 (Gradle), 238 (Bukkit), 378 (Java bindings), 215 (API crate)
- [ ] L6 `mcchanges_download` option (download the jar with curl): never built, not asked again
- [ ] L7 Linux and macOS programs in the release: the CI way (`release.py tag`) makes them
- [ ] L8 Cross-check Java tokens between the builder and PackSquash with real Java data (Mojang was not reachable from the AI sandbox)

## Done

- [x] 0001 to 0008: skip files same as vanilla (mcchanges), versions, Bedrock, language keys, sky images, `copy_as_is`, shader `#include`, audio hashes
- [x] 0010: hash tokens (`png:`, `json:`, `langkeys:`), in master as "mc-changes ready"
- [x] 0011 release prep (0.5.0-fork.1, runners, no wiki sync). 0012 task list. 0013 and 0015 dependency fixes. 0014 compact language keys (`langfull:`, `langdelta:`)
- [x] Tools outside the repository: `build_mcchanges_jar.py` (compact, 40% of the threads, reuses `C:\amc\work`), `old_build_mcchanges_jar.py`, `release.py`, `apply_patches.py`, READMEs, `mc-changes` README
- [x] t1: Bedrock jars have no folders because they have no language files to store (see the answer in chat)

## Rules

- Scripts and their READMEs are separate files, never in a patch. A change of this file goes inside another patch.
- Send each patch or file when it is finished, then go on with this list. Send this file again after progress.
- Simple English. Windows 11 and cmd commands.
