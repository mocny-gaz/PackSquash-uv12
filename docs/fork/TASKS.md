# Task list of the fork

Updated 2026-11-04. Edit it freely and upload it. One line per task: `- [ ] ID text (who)`. Put long text, like an error, in a
block under the task. Fixed things move to Done, in one short line.

## This very moment

- [ ] r1fix this appearing on official release (not prerelease): `McChanges pack error: mcchanges_version is set, but the mcchanges pack does not hold several game versions`. STATE: the jar from the `mc-changes` Release works here with `mcchanges_version = '1.8.9'` (87 versions). So `mcchanges_path` in your options file points to something else, most likely a folder or a game jar. Patch 0019 makes the error name the path and say what it is, and for a folder it lists the jars in it. Build it, run again, and tell me the new message if it still fails (owner)
- [ ] r2 make it log a troubleshoot if error by default or log or anything helpful. STATE: patch 0019 makes every error print a `How to fix` line for its likely cause, a link to the fork docs (`USAGE.md#common-problems`) and how to get a debug log (`set PACKSQUASH_LOG=debug`). `mcchanges_path` errors name the path. The list of causes is in `docs/fork/USAGE.md` (owner)

## Features (in progress: the AI prepares the patches, the owner builds, then applies)

- [ ] F1 Any archive format as input (`.7z`, `.tar`, `.tar.gz`, `.tar.xz`, `.tar.zst`, `.tar.bz2`, `.zip`, `.rar` through a helper program, others), found by looking at the contents: `pack.mcmeta` with `assets/`, or a Bedrock `manifest.json`, also inside a subfolder. The output stays a zip. STATE: patch 0020 is written and tested here (unit tests and 34 end to end runs). Not tested: real RAR files and Windows `tar.exe`. Apply after 0019, build, then try your own archives, also a `.rar` (owner)
- [ ] F2 Bulk squash: every pack found in a directory, whatever it looks like, also when it is an archive of any kind. Output next to the input with the prefix `squashed_` by default; options for a custom prefix and suffix, an overwrite option (keep a backup unless told not to), a dry run and a report. STATE: patch 0021 is written and tested here (unit tests and 13 end to end runs). Apply after 0020, build, then try `bulk_dry_run = true` on a copy of your packs first (owner)
- [ ] f4 make auto detect (run against every version of mcchanges) when mcchanges_version = is left blank. STATE: designed, not written (AI)
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

- [x] r1 patch 0017 (raw line breaks and tabs inside JSON strings), the TOML builder Artifact, and the README index with `docs/fork/USAGE.md` (patch 0018)
- [x] 0001 to 0008: skip files same as vanilla (mcchanges), versions, Bedrock, language keys, sky images, `copy_as_is`, shader `#include`, audio hashes
- [x] 0010: hash tokens (`png:`, `json:`, `langkeys:`), in master as "mc-changes ready"
- [x] 0011 to 0016 applied: release prep, task list, dependency fixes, compact language keys (`langfull:`, `langdelta:`), no `generic_const_exprs`
- [x] First release 0.5.0-fork.1 (GNU build with WinLibs, `release.py`). `cargo fmt` run. Jars rebuilt and published as Release files in `mc-changes`
- [x] Dependabot alerts fixed. `--include-previews` crash fixed. `msvcrt.dll` check fixed
- [x] Tools outside the repository: `build_mcchanges_jar.py` (compact, 40% of the threads, reuses `C:\amc\work`), `old_build_mcchanges_jar.py`, `release.py`, `apply_patches.py`, READMEs, `mc-changes` README
- [x] t1: Bedrock jars have no folders because they have no language files to store

## Rules

- Scripts and their READMEs are separate files, never in a patch. A change of this file goes inside another patch.
- Send each patch or file when it is finished, then go on with this list. Send this file again after progress, cleaned: no pasted logs, fixed things only in Done.
- Simple English. Windows 11 and cmd commands.
