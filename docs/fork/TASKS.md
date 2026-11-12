# Task list of the fork

Updated 2026-11-11. Edit it freely and upload it. One line per task: `- [ ] ID text (who)`. Put long text, like an error, in a
block under the task. Fixed things move to Done, in one short line. Tasks are sorted by ID. STATE is what the AI did.

## Bugs

- [ ] b1 when using bulk input as a zip it did only 20 out of 204 that there were and just spammed the folder that zip was in (make r14) also .7z as an input doesnt work either (also r8 and r12) (ai) STATE: patch 0030 (sent). Archives inside archives are opened (3 levels), results go in a folder that mirrors the inside, and progress prints as each pack ends. Tested here with a zip of 204 packs (184 zips and 20 folders) and a 7z that has only zips. Waiting for your real run
- [ ] b2 still things like .py and other get skipped in other mods folders so make it optimise but never skip no matter what (unless damaged, dont make it false flag obfuscation as damaged) (ai) STATE: patch 0029 (sent again, rebased on your master). `include_unrecognized_asset_files` is on by default, and `copy_failed_files` (on by default) copies a file that fails to optimize as it is, with a notice. Only a failure before anything of the file was written can be copied
- [ ] b3 files that fail to optimize failed whole packs: `.ogg` files that are not Vorbis, `pack.png` that is not a PNG, PNG too big, invalid `.json` and `.lang` files (36 of 204 packs failed in one run) (ai) STATE: same patch as b2 (0029)
- [ ] b4 29 zip files could not be read: `Multi-disk ZIP files are not supported` (ai) STATE: patch 0030 (sent). The zip reader gives them to the helper program (7z, bsdtar, tar), like RAR. Not tested here with a real multi-disk zip or with 7-Zip (my sandbox has no 7-Zip): only the decision to use the helper and the error text are tested. If no helper reads it, the error says why and what to install
- [ ] b5 bulk size line is wrong, like `24061.29 MiB -> 7.77 MiB` (ai) STATE: patch 0030 (sent). The size is the size of the pack's files
- [ ] b6 bulk labels should show the pack name and fuller directory (this is f5), and the log of the bulk run is hard to read when an archive has many packs (ai) STATE: patch 0030 (sent). Labels are `[3/204] Pack (C:\in\big.zip > folder\inner.zip > Pack)`. The debug log has a line for every pack with its folder and result

## This very moment

- [ ] r1 make it so that debug log name also contains the input directory name (or bulk input directory name) (ai) STATE: patch 0031 (sent). The default log is `packsquash-debug-NAME.log`, where NAME is the name of the pack directory, the archive (no extension) or the bulk input directory. `log_file` still sets any path you want
- [ ] r2 optifine properties(sky models and others) optimization - if for example sky3.properties is about aurora.png but there isnt such file then skip it (name that module optifine sky debloat/fix and put a warning that on wip packs it can lead to progress loss (if the aurora wasnt YET added or sth) so dont use it on unfinished ones or dev ones and it can fix sky problems when transforming pack to other versions (in old optifine if there wasnt a texture for sky it was just skipped but in new it does the pink/black error texture)). By r2 i mean that any bloat in optifine that isnt connected to something (isnt used in game with pack loaded) (ai) STATE: custom sky entries are done (patch 0023, applied). Next: the other OptiFine files that are not connected to anything (connected textures, custom items and the rest)
- [ ] r3 update the artifact to match new addition (ai) STATE: updated for `bulk_output_directory` (same link as before). One more update after r11
- [ ] r6 decide a better design as an extension of packsquash or wtv feels better for packsquashes purpose(just the idea) for the f6 and l9 (ai)
- [ ] r7 make mc-changes auto download and delete after use (unless no wifi then check if in folder(the one where the packsquash and the one where input or the one where output) and if not in folder type in cmd that no mcchanges jar skipping that step and does the normal one) by default (ai) STATE: not done, with L6
- [ ] r11 make all the long options names (like the skip_optifine_sky_entries_with_missing_images make it some like optifine sky debloat/fix or anything else thats shorter and makes sense and doesnt collide with other options) way shorter (ai) STATE: next patch (0031). Plan: shorter names, and the old names keep working as serde aliases, so no options file breaks
- [ ] r14 make bulk output option (if its set to sth other than default or same as input then put there without prefix (unless other than default)) (ai) STATE: patch 0030 (sent). Option `bulk_output_directory`. The results keep the folders of the input and have no prefix unless you set `bulk_output_prefix`. Same folder as the input means not set. `bulk_overwrite` does nothing with it

## Features (in progress: the AI prepares the patches, the owner builds, then applies)

- [ ] F2 Bulk squash: every pack found in a directory, whatever it looks like, also when it is an archive of any kind. Output next to the input with the prefix `squashed_` by default; options for a custom prefix and suffix, an overwrite option (keep a backup unless told not to), a dry run and a report. STATE: patch 0021 (applied). Patch 0030 (sent) has the b1 fixes it needed
- [ ] F3 Very simple GUI like Win11Debloat, only dark OLED mode, easy settings, export and import of the settings. Later release, last of all (ai)
- [ ] f5 when bulk show also packs name/fuller directory (ai) STATE: patch 0030 (sent), see b6
- [ ] f6 a fixer which will when fixer = true fix common detected issues in resourcepack *only when a pack/file failed* (ai)

## Later

- [ ] L1 Issue 119: shader minification. Needs study
- [ ] L2 Issue 362: function inlining. Needs study
- [ ] L3 Issue 300: Nemo's Transit Expansion. Needs study
- [ ] L4 Upstream issues 254 (overlays) and 373 (formats): wait for upstream
- [ ] L5 Not planned: 196 (Gradle), 238 (Bukkit), 378 (Java bindings), 215 (API crate)
- [ ] L6 `mcchanges_download` option (download the jar with curl): never built, not asked again UPDATE: upgraded to a feature and its supposed to be a default if no mcchanges found or theyre older also can be overwriten with mcchanges_nowifi and then it just searches for it and if no connection then auto just search in folder and if there wasnt any mcchanges in folder and you downloaded then after squashing remove the mcchanges (unless mcchanges_nowifi then keep them)
- [ ] L7 Linux and macOS programs in the release: the CI way (`release.py tag`) makes them
- [ ] L8 Cross-check Java tokens between the builder and PackSquash with real Java data (Mojang was not reachable from the AI sandbox)
- [ ] L9 when f3 gui will be made at around that part of development and when f6 be implemented make a mode at the bottom of gui to just FIX if any issues with packs aand it will just check everything and then a prompt will appear to apply changes (bypassable with fixer_prompt = false)

## Done

- [x] 0001 to 0008: skip files same as vanilla (mcchanges), versions, Bedrock, language keys, sky images, `copy_as_is`, shader `#include`, audio hashes
- [x] 0010: hash tokens (`png:`, `json:`, `langkeys:`), in master as "mc-changes ready"
- [x] 0011 to 0016 applied: release prep, task list, dependency fixes, compact language keys (`langfull:`, `langdelta:`), no `generic_const_exprs`
- [x] First release 0.5.0-fork.1 (GNU build with WinLibs, `release.py`). `cargo fmt` run. Jars rebuilt and published as Release files in `mc-changes`
- [x] Dependabot alerts fixed. `--include-previews` crash fixed. `msvcrt.dll` check fixed
- [x] Tools outside the repository: `build_mcchanges_jar.py` (compact, 40% of the threads, reuses `C:\amc\work`), `old_build_mcchanges_jar.py`, `release.py`, `apply_patches.py`, READMEs, `mc-changes` README
- [x] t1: Bedrock jars have no folders because they have no language files to store
- [x] Patch 0017 (raw line breaks and tabs inside JSON strings), the TOML builder Artifact, and the README index with `docs/fork/USAGE.md` (patch 0018)
- [x] r1fix and the old r2: clear `mcchanges_path` errors, a `How to fix` line and links after every error (patch 0019)
- [x] F1 any archive format as input (patch 0020). Not tested: real RAR files and Windows `tar.exe`
- [x] f4 auto detect of the game version when `mcchanges_version` is blank (patch 0022)
- [x] r4 `mcchanges_path` can be a folder with `mcchanges-java.jar` (or `mcchanges-bedrock.jar`) by exact name (patch 0024)
- [x] old r1 and r9: no file or folder under `assets/minecraft` or any `assets/NAME` is skipped (patch 0025)
- [x] r5 and r10: `log = true` writes a very detailed debug log (patch 0027). Your logs showed it works
- [x] r8: an archive file as `bulk_input_directory` (patch 0026)
- [x] r13: a blank `mcchanges_version` compares each file with the same file in every version that changed it (patch 0028, in your master as "mcchanges improvements")

## Rules

- Scripts and their READMEs are separate files, never in a patch. A change of this file goes inside another patch.
- Send each patch or file when it is finished, then go on with this list. Send this file again after progress, cleaned: no pasted logs, fixed things only in Done.
- Simple English. Windows 11 and cmd commands.
- Clean up after user and sort and order each time
- delete comments on resend. if answer is asked just make another comment and delete old one
