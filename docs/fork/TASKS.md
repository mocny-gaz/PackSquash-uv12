# Task list of the fork

Updated 2026-11-12. Edit it freely and upload it. One line per task: `- [ ] ID text (who)`. Put long text, like an error, in a
block under the task. Fixed things move to Done, in one short line. Tasks are sorted by ID. STATE is what the AI did.

## Bugs

- [done ] b5 bulk size line is wrong, like `24061.29 MiB -> 7.77 MiB` (ai) STATE: patch 0030 made it the size of the pack's files. Patch 0034 (sent): a pack that is a whole archive (a zip inside a zip) counts as the size of that archive file, so both sizes are compressed. A pack in a folder of a bigger archive still counts its extracted files, because its compressed size is not known
- [ check yourself] b7 potential bug because i get a lot of "could not be optimized (Invalid PNG: The texture width or height exceeds the configured maximum size. More information: <https://packsquash.aylas.org/links/Too-big-PNG-help>), so it was copied as it is" so it may apply to more than it tells but im unsure its a possibility that its just my case but i see a lot of these on shulkerbox entities (and other too) (ai) STATE: patch 0032 (sent). The check is `maximum_width_and_height` (8192 by default) and it is correct: your logs have `fire_0.png`, `fire_1.png` (tall strips of animation frames) and `seagrass.png`. The message now says the real size, like `The texture is 16x9600 pixels`. Run again and read the sizes. If a shulker texture is 8192 or smaller, tell me the file

## This very moment

- [done ] r1 make it so that debug log name also contains the input directory name (or bulk input directory name) (ai) STATE: patch 0031 (sent). The default log is `packsquash-debug-NAME.log`, where NAME is the name of the pack directory, the archive (no extension) or the bulk input directory
- [ ] r2 optifine properties(sky models and others) optimization - if for example sky3.properties is about aurora.png but there isnt such file then skip it (name that module optifine sky debloat/fix and put a warning that on wip packs it can lead to progress loss (if the aurora wasnt YET added or sth) so dont use it on unfinished ones or dev ones and it can fix sky problems when transforming pack to other versions (in old optifine if there wasnt a texture for sky it was just skipped but in new it does the pink/black error texture)). By r2 i mean that any bloat in optifine that isnt connected to something (isnt used in game with pack loaded) (ai) STATE: custom sky entries are done (patch 0023, applied). Next: the other OptiFine files that are not connected to anything (connected textures, custom items and the rest)
- [ ] r3 update the artifact to match new addition (ai) STATE: updated for `bulk_output_directory` (same link as before). One more update after r11
- [ ] r6 decide a better design as an extension of packsquash or wtv feels better for packsquashes purpose(just the idea) for the f6 and l9 (ai) note: when getting to that point tell owner to send the idea files
- [ ] r7 make mc-changes auto download and delete after use (unless no wifi then check if in folder(the one where the packsquash and the one where input or the one where output) and if not in folder type in cmd that no mcchanges jar skipping that step and does the normal one) by default; also make it auto clear after each run all temp resources or anything left to consume itself least space (ai) STATE: not done, with L6
- [ ] r11 make all the long options names (like the skip_optifine_sky_entries_with_missing_images make it some like optifine sky debloat/fix or anything else thats shorter and makes sense and doesnt collide with other options) way shorter (ai) STATE: next patch (0035). Plan: shorter names, and the old names keep working as serde aliases, so no options file breaks
- [ ] r15 make it so that the bulk archive depth also is how many archives in archives depth (ai) STATE: patch 0033 (sent). I used `bulk_scan_depth` (default 32; `archive_pack_search_depth` counts if it is higher), at most 32. Tell me if you meant another option

## Features (in progress: the AI prepares the patches, the owner builds, then applies)

- [ ] F3 Very simple GUI like Win11Debloat, only dark OLED mode, easy settings, export and import of the settings. Later release, last of all (ai)
- [ ] f6 a fixer which will when fixer = true fix common detected issues in resourcepack *only when a pack/file failed* (ai)
- [do this NOW(i upgrade it to r1) ] r1(f7) make results not folder but an archive (same type as input, but with squashed prefix unless in other directory where there isnt any with such name) (ai) STATE: not started. A zip, tar, tar.gz and the like input is possible. A 7z input needs the `compress` feature of `sevenz-rust2` in the normal build. A RAR can not be written by anyone but WinRAR, so a RAR input would give a zip. The results inside are already zips, so they would be stored, not compressed again note: when packing it always pack them up as much as possible (like if 7zip then ultra best savings settings but for every(also when .zip then make it the max that windows itself without any additional software can unpack)) also if bulk input is a folder then make it pack into folder with same name (in output bulk, unless it'd be a duplicate then do squashed_ just on folder, also if same happens then for zips too the squashed_ prefix)

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
