# Task list of the fork

Updated 2026-11-05. Edit it freely and upload it. One line per task: `- [ ] ID text (who)`. Put long text, like an error, in a
block under the task. Fixed things move to Done, in one short line.

## This very moment

- [ ] r2 optifine properties(sky models and others) optimization - if for example sky3.properties is about aurora.png but there isnt such file then skip it (name that module optifine sky debloat/fix and put a warning that on wip packs it can lead to progress loss (if the aurora wasnt YET added or sth) so dont use it on unfinished ones or dev ones and it can fix sky problems when transforming pack to other versions (in old optifine if there wasnt a texture for sky it was just skipped but in new it does the pink/black error texture)) (ai) STATE: patch 0023 does the custom sky part: option `skip_optifine_sky_entries_with_missing_images`, with the warning, tested here (unit tests and a run with the CLI). It keeps an entry that a later numbered entry needs, because OptiFine stops loading at the first missing number. Not done yet: the other OptiFine properties (CTM, CIT). Apply after 0022 (owner)
- [ ] r4 make mcchanges path ALSO accept if its just a folder and in that folder it has to find the mcchanges (dont make this overcomplicated just make it so that it has to match the exact name AND manifest or wtv)
- [ ] r3 update the artifact  to match new addition
- [ ] r1 make it not skip any folders or files if in /minecraft (for mods compatibility)
- [ ] r5 with log = true make it write a debug log writing down everything so that i can send it over in order to fix stuff (like a general one for fixing or debugging or comparing purposes make it contain ALL the data of workflow and stuff very detailed like which hash was applied to which file in which pack and that level of detail stuff)
- [ ] r6 decide a better design as an extension of packsquash or wtv feels better for packsquashes purpose(just the idea) for the f6 and l9

## Features (in progress: the AI prepares the patches, the owner builds, then applies)

- [ ] F1 Any archive format as input (`.7z`, `.tar`, `.tar.gz`, `.tar.xz`, `.tar.zst`, `.tar.bz2`, `.zip`, `.rar` through a helper program, others), found by looking at the contents: `pack.mcmeta` with `assets/`, or a Bedrock `manifest.json`, also inside a subfolder. The output stays a zip. STATE: patch 0020 is written and tested here (unit tests and 34 end to end runs). Not tested: real RAR files and Windows `tar.exe`. Apply after 0019, build, then try your own archives, also a `.rar` (owner)
- [ ] F2 Bulk squash: every pack found in a directory, whatever it looks like, also when it is an archive of any kind. Output next to the input with the prefix `squashed_` by default; options for a custom prefix and suffix, an overwrite option (keep a backup unless told not to), a dry run and a report. STATE: patch 0021 is written and tested here (unit tests and 13 end to end runs). Apply after 0020, build, then try `bulk_dry_run = true` on a copy of your packs first (owner)
- [ ] f4 make auto detect (run against every version of mcchanges) when mcchanges_version = is left blank. STATE: patch 0022 is written and tested here (unit tests, a squash test, 8 end to end runs with your real `mcchanges-java.jar`). Apply after 0021. Try it with the version left out, and read the line that tells which version it chose (owner)
- [ ] F3 Very simple GUI like Win11Debloat, only dark OLED mode, easy settings, export and import of the settings. Later release, last of all (AI)
- [ ] f5 when bulk show also packs name/fuller directory
- [ ] f6 a fixer which will when fixer = true fix common detected issues in resourcepack *only when a pack/file failed*
## Later

- [ ] L1 Issue 119: shader minification. Needs study
- [ ] L2 Issue 362: function inlining. Needs study
- [ ] L3 Issue 300: Nemo's Transit Expansion. Needs study
- [ ] L4 Upstream issues 254 (overlays) and 373 (formats): wait for upstream
- [ ] L5 Not planned: 196 (Gradle), 238 (Bukkit), 378 (Java bindings), 215 (API crate)
- [ ] L6 `mcchanges_download` option (download the jar with curl): never built, not asked again UPDATE: upgraded to a feature and its supposed to be a default if no mcchanges found or theyre older also can be overwriten with mcchanges_nowifi and then it just searches for it and if no connection then auto just search in folder and if there wasnt any mcchanges in folder and you downloaded then after squashing remove the mcchanges (unless mcchanges_nowifi then keep them)
- [ ] L7 Linux and macOS programs in the release: the CI way (`release.py tag`) makes them
- [ ] L8 Cross-check Java tokens between the builder and PackSquash with real Java data (Mojang was not reachable from the AI sandbox)
- [ ] l9 when f3 gui will be made at around that part of development and when f6 be implemented make a mode at the bottom of gui to just FIX if any issues with packs aand it will just check everything and then a prompt will appear to apply changes (bypassable with fixer_prompt = false)
## Done

- [x] Patch 0017 (raw line breaks and tabs inside JSON strings), the TOML builder Artifact, and the README index with `docs/fork/USAGE.md` (patch 0018)
- [x] r1fix and the old r2: clear `mcchanges_path` errors, a `How to fix` line and links after every error (patch 0019)
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
