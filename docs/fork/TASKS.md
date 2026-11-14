# Task list of the fork

Updated 2026-11-13. Edit it freely and upload it. One line per task: `- [ ] ID text (who)`. Put long text, like an error, in a
block under the task. Fixed things move to Done, in one short line. Tasks are sorted by ID. STATE is what the AI did.

## Bugs

None open.


## This very moment

- [ ] r2 optifine properties(sky models and others) optimization - if for example sky3.properties is about aurora.png but there isnt such file then skip it (name that module optifine sky debloat/fix and put a warning that on wip packs it can lead to progress loss (if the aurora wasnt YET added or sth) so dont use it on unfinished ones or dev ones and it can fix sky problems when transforming pack to other versions (in old optifine if there wasnt a texture for sky it was just skipped but in new it does the pink/black error texture)). By r2 i mean that any bloat in optifine that isnt connected to something (isnt used in game with pack loaded) (ai) STATE: custom sky entries are done (patch 0023, applied). Next: the other OptiFine files that are not connected to anything (connected textures, custom items and the rest)
- [ ] r3 update the artifact to match new addition (ai) STATE: updated for `bulk_output_directory` (same link as before). One more update after r11
- [ ] r6 decide a better design as an extension of packsquash or wtv feels better for packsquashes purpose(just the idea) for the f6 and l9 (ai) note: when getting to that point tell owner to send the idea files
- [ ] r7 make mc-changes auto download and delete after use (unless no wifi then check if in folder(the one where the packsquash and the one where input or the one where output) and if not in folder type in cmd that no mcchanges jar skipping that step and does the normal one) by default; also make it auto clear after each run all temp resources or anything left to consume itself least space (ai) STATE: not done, with L6
- [ ] r11 make all the long options names (like the skip_optifine_sky_entries_with_missing_images make it some like optifine sky debloat/fix or anything else thats shorter and makes sense and doesnt collide with other options) way shorter (ai) STATE: next patch (0036). Plan: shorter names, and the old names keep working as serde aliases, so no options file breaks
- [ ] r15 make it so that the bulk archive depth also is how many archives in archives depth (ai) STATE: patch 0033 (sent). I used `bulk_scan_depth` (default 32; `archive_pack_search_depth` counts if it is higher), at most 32. Tell me if you meant another option

## Features (in progress: the AI prepares the patches, the owner builds, then applies)

- [ ] F3 Very simple GUI like Win11Debloat, only dark OLED mode, easy settings, export and import of the settings. Later release, last of all (ai)
- [ ] f6 a fixer which will when fixer = true fix common detected issues in resourcepack *only when a pack/file failed* (ai)
- [ ] r1(f7) make results not folder but an archive (same type as input, but with squashed prefix unless in other directory where there isnt any with such name) (ai) note: always pack as much as possible (7zip ultra, zip the max that windows can unpack without additional software); if the bulk input is a folder, pack into a folder with the same name in the output (squashed_ on the folder if it exists, and the same for zips) STATE: patch 0035 (sent). An archive with several packs or with archives inside gives an archive of the same kind: zip, 7z, tar, tar.gz and tar.bz2 are written, a RAR input gives a zip. Zip is Deflate level 9 (Windows opens it alone), 7z is LZMA2 level 9. Results are added to the archive one by one, so the disk holds only one result at a time. With `bulk_output_directory`, the results go in a folder named like the input folder (`squashed_` if it exists), and a result that exists gets `squashed_`. Tested with tar, tgz, tar.bz2, zip, 7z and a zip of 204 packs

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

- [x] b5: the bulk size line (patches 0030 and 0034). b7: not a bug. The 16 shulker files in your logs are about 1.5 MB each (vanilla is about 2 KB), so they really are bigger than 8192 pixels. The message shows the size now (patch 0032). Raise `maximum_width_and_height` to optimize them. old r1 (debug log name, patch 0031) and r15 (patch 0033)
- [x] b1, r8, r12, r14, F2, f5, b6: bulk mode opens archives inside archives, results in a folder or in `bulk_output_directory`, labels with the pack name and where it is (patches 0026, 0030). You marked them fixed
- [x] b2 and b3: files that fail to optimize are copied as they are, and nothing under `assets` is skipped (patch 0029). Checked against your logs: a pack with every kind of failure in them (`.ogg` that is not Vorbis, MP4 audio named `.ogg`, `pack.png` that is not a PNG, PNG too big, invalid `.json`, `.lang`, a `.py` in a mod folder) squashes with exit code 0 and every file is kept
- [x] b4: a zip the zip reader can not read (multi-disk) goes to the helper program (patch 0030). You marked it fixed
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
- give patches as soon as theyre done and continue moving with the task list, if theres sth which needs users info or choice then ask it and go on with the list not wasting resources
- obey .rules but prioritise user
- prioritise user sent TASKS.md over this file
