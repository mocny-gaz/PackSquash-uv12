# Task list of the fork

Updated 2026-11-09. Edit it freely and upload it. One line per task: `- [ ] ID text (who)`. Put long text, like an error, in a
block under the task. Fixed things move to Done, in one short line. Tasks are sorted by ID. STATE is what the AI did.

## Bugs

- [ ] b1 when using bulk input as a zip it did only 20 out of 204 that there were and just spammed the folder that zip was in (make r14) also .7z as an input doesnt work either (ai) STATE: cause found in the logs. 184 of the 204 packs are zip files inside the zip (and the 7z has only zips inside it), and PackSquash does not open archives inside archives. The 20 it found are packs stored as folders. Also the results go next to the archive, with long names. Designed, not written: nested archives, results in a folder that mirrors the inside of the archive, and r14. Details in HANDOFF.txt
- [ ] b2 still things like .py and other get skipped in other mods folders so make it optimise but never skip no matter what (unless damaged, dont make it false flag obfuscation as damaged) (ai) STATE: cause found. `include_unrecognized_asset_files` was off by default, and files that fail to optimize failed the whole pack. In patch 0029 (tested here): it is on by default, and `copy_failed_files` (on by default) copies a file that fails to optimize as it is, with a notice. Only a failure before anything of the file was written can be copied
- [ ] b3 files that fail to optimize failed whole packs: `.ogg` files that are not Vorbis (some are MP4 audio, error `isomp4: missing moov atom`), `pack.png` that is not a PNG (24 packs), PNG too big, invalid `.json` and `.lang` files (found in the logs: 36 of 204 packs failed in one run) (ai) STATE: fixed by `copy_failed_files` in patch 0029, same as b2
- [ ] b4 29 zip files could not be read: `Multi-disk ZIP files are not supported` (the zip library refuses them, tools like 7-Zip and bsdtar read them) (ai) STATE: not done. Plan: when the zip library fails on the structure of a zip file, read it with the helper program (7z, bsdtar, tar), like RAR
- [ ] b5 bulk size line is wrong, like `24061.29 MiB -> 7.77 MiB`: it counts the size of the whole archive for each pack inside it (ai) STATE: not done. Plan: use the size of the extracted pack folder
- [ ] b6 bulk labels should show the pack name and fuller directory (this is f5), and the log of the bulk run is hard to read when an archive has many packs (ai) STATE: not done, with f5

## This very moment

- [ ] r1 make it not skip any folders or files if in /minecraft (for mods compatibility) (ai) STATE: patch 0025 (applied). See b2: the option is now on by default in 0029
- [ ] r2 optifine properties(sky models and others) optimization - if for example sky3.properties is about aurora.png but there isnt such file then skip it (name that module optifine sky debloat/fix and put a warning that on wip packs it can lead to progress loss (if the aurora wasnt YET added or sth) so dont use it on unfinished ones or dev ones and it can fix sky problems when transforming pack to other versions (in old optifine if there wasnt a texture for sky it was just skipped but in new it does the pink/black error texture)). By r2 i mean that any bloat in optifine that isnt connected to something (isnt used in game with pack loaded) (ai) STATE: custom sky entries are done (patch 0023, applied). Next, after the log tasks: the other OptiFine files that are not connected to anything (connected textures, custom items and the rest)
- [ ] r3 update the artifact  to match new addition (ai) STATE: done, the artifact was updated twice (log, sky, asset, folder options). Link in the chat. It needs one more update for r11 and r14
- [ ] r5 with log = true make it write a debug log writing down everything so that i can send it over in order to fix stuff (like a general one for fixing or debugging or comparing purposes make it contain ALL the data of workflow and stuff very detailed like which hash was applied to which file in which pack and that level of detail stuff) (ai) STATE: patch 0027 (applied). The owner sent 4 logs, which were analysed (b1 to b6)
- [ ] r6 decide a better design as an extension of packsquash or wtv feels better for packsquashes purpose(just the idea) for the f6 and l9 (ai)
- [ ] r7 make mc-changes auto download and delete after use (unless no wifi then check if in folder(the one where the packsquash and the one where input or the one where output) and if not in folder type in cmd that no mcchanges jar skipping that step and does the normal one) by default (ai)
- [ ] r8 bulk squash when .7z and any other archive just says that it isnt a directory (ai) STATE: patch 0026 (applied) accepts an archive file. The 7z has only zip files inside it, so see b1 (nested archives)
- [ ] r9 make so that NO files in assets/(anyfoldername) get skipped as this can break mods (ai) STATE: patch 0025 (applied). See b2
- [ ] r10 update the options artifact with the log thing and implement log if you haent already and make it log every detail i said (from detailed error reports from why cant open zip thru detailed errors why vorbis didnt work to which hash on which file which pack was used so basically very very detailed on everything that can fail for debugging and fixing purposes) (ai) STATE: patch 0027 (applied) and the artifact are done. The logs showed that it works (the Vorbis error shows the library, the function and the kind)
- [ ] r11 make all the long options names (like the skip_optifine_sky_entries_with_missing_images make it some like optifine sky debloat/fix or anything else thats shorter and makes sense and doesnt collide with other options) way shorter (ai) STATE: not done. Plan: shorter names with the old names kept as serde aliases, so no options file breaks
- [ ] r12 bulk scan somehow capped at 204 so fix it so that it isnt capped (ai) STATE: it was not a cap: see b1 (zip files inside the archive are not opened)
- [ ] r13 when mcchanges version empty make mcchanges compare each hash with same file name on each version (lets say we have dirt.png so compare it against every hash of dirt.png from mcchanges (so on each version that had any hash/file changes), NOT just preset 1 version and compare there, NOT compare against every hash of every file on every version) STATE: patch 0028 (sent). A blank version compares each file with the same file in every version that changed it. `auto` is the one best version (f4)
- [ ] r14 make bulk output option (if its set to sth other than default or same as input then put there without prefix (unless other than default)) (ai) STATE: not done. Plan: option `bulk_output_directory`. If it is set and is not the input folder, the results go there, with the same folders as the input, and without the prefix unless the owner sets `bulk_output_prefix` himself. See HANDOFF.txt

## Features (in progress: the AI prepares the patches, the owner builds, then applies)

- [ ] F1 Any archive format as input (`.7z`, `.tar`, `.tar.gz`, `.tar.xz`, `.tar.zst`, `.tar.bz2`, `.zip`, `.rar` through a helper program, others), found by looking at the contents: `pack.mcmeta` with `assets/`, or a Bedrock `manifest.json`, also inside a subfolder. The output stays a zip. STATE: patch 0020 (applied). The owner uses it with 7z and zip. Not tested: real RAR files and Windows `tar.exe`
- [ ] F2 Bulk squash: every pack found in a directory, whatever it looks like, also when it is an archive of any kind. Output next to the input with the prefix `squashed_` by default; options for a custom prefix and suffix, an overwrite option (keep a backup unless told not to), a dry run and a report. STATE: patch 0021 (applied). Needs the b1 fixes (nested archives, output folders)
- [ ] F3 Very simple GUI like Win11Debloat, only dark OLED mode, easy settings, export and import of the settings. Later release, last of all (ai)
- [ ] f4 make auto detect (run against every version of mcchanges) when mcchanges_version = is left blank. STATE: patch 0022 (applied)
- [ ] f5 when bulk show also packs name/fuller directory (ai) STATE: not done, with b6
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
- [x] r4 `mcchanges_path` can be a folder with `mcchanges-java.jar` (or `mcchanges-bedrock.jar`) by exact name and a `mcchanges.json` inside (patch 0024)

## Rules

- Scripts and their READMEs are separate files, never in a patch. A change of this file goes inside another patch.
- Send each patch or file when it is finished, then go on with this list. Send this file again after progress, cleaned: no pasted logs, fixed things only in Done.
- Simple English. Windows 11 and cmd commands.
- Clean up after user and sort and order each time
- delete comments on resend. if answer is asked just make another comment and delete old one