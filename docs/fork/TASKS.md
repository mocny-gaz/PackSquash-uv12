# Task list of the fork

Updated 2026-11-21. Edit it freely and upload it. One line per task: `- [ ] ID text (who)`. Put long text, like an error, in a
block under the task. Fixed things move to Done, in one short line. Tasks are sorted by ID. STATE is what the AI did.

## Bugs

- [ ] b9 `Temmie.zip`: one `.lang` file with a line without `=` fails the whole pack (ai) STATE: patch 0046 (in your master). The file is copied as it is. Tested on a real pack
- [ ] b10 12 of 46 Modrinth packs fail because of the checks of `pack.mcmeta` (`supported_formats` with `min_format`, three numbers in a version, a decimal number) (ai) STATE: patch 0047 (in your master). The pack is squashed anyway, with a notice, when `copy_failed_files` is on. Tested with the exact error of the log

## This very moment

- [ ] r1 auto pack.mcmeta fixer as an option enabled by default (ai) STATE: waiting for your answers (questions 3 to 5 in chat). No code yet

## Features (in progress: the AI prepares the patches, the owner builds, then applies)

- [ ] F3 Very simple GUI like Win11Debloat, only dark OLED mode, easy settings, export and import of the settings. Later release, last of all. Open it when double clicking packsquash, also add a mode for cli in that gui (ai) STATE: patch 0042 (in your master). Patch 0050 (sent): on Windows, double clicking `packsquash.exe` opens the window; a terminal start with no arguments works as before. Not tested: a real double click and the page in a real browser, my sandbox has neither. CLI mode: waiting for your answer (question 1 in chat)
- [ ] f6 a fixer which will when fixer = true fix common detected issues in resourcepack *only when a pack/file failed* (ai) STATE: the JSON fixer is built (patches 0043 and 0044, kept ready, not applied). Waiting for your answers in `R6-DESIGN-FOR-YOUR-OPINION.md` (questions 6 to 8 in chat)

## Later

- [ ] L1 Issue 119: shader minification. Needs study STATE: study note sent. My pick is to skip or to measure a pack first. No code until you answer (question 9 in chat)
- [ ] L2 Issue 362: function inlining. Needs study STATE: study note sent. My pick is to skip, or an opt-in safe subset only if you use data packs. No code until you answer (question 10 in chat)
- [ ] L3 Issue 300: Nemo's Transit Expansion. Needs study STATE: study note sent. Already works in the fork: `.obj`, `.mtl` and `.js` under `assets/mtr` are kept (tested, with and without the MTR option). I propose to close it. Please test one real NTE pack (question 11 in chat)
- [ ] L4 Upstream issues 254 (overlays) and 373 (formats): wait for upstream
- [ ] L5 Not planned: 196 (Gradle), 238 (Bukkit), 378 (Java bindings), 215 (API crate)
- [ ] L7 Linux and macOS programs in the release: the CI way (`release.py tag`) makes them
- [ ] L8 Cross-check Java tokens between the builder and PackSquash with real Java data (Mojang was not reachable from the AI sandbox)
- [ ] L9 when f3 gui will be made at around that part of development and when f6 be implemented make a mode at the bottom of gui to just FIX if any issues with packs aand it will just check everything and then a prompt will appear to apply changes (bypassable with fixer_prompt = false)

## Done

- [x] b8, b11: not bugs. Patch 0048 is reverted (patch 0049). r6: design file sent. r17, r18, r20: release script (normal release, window test, free tags). f7: results are an archive of the same kind as the input (patch 0035)
- [x] r19: `release.py build --tester` makes a local test build with no tag and no internet. You said it works. r18: the release script tests the window by itself
- [x] r3: the TOML builder page has the new names and options. r11: shorter option names (patch 0040). r16: release prep (patch 0041) and the Dependabot check. The only change is `soupsieve 4.0` in `scripts/test-packs/requirements.txt`. It resolves with the pinned `beautifulsoup4`, and no Rust file changed. You marked them done
- [x] r2: `optifine_unused` leaves out OptiFine entries that are not connected to anything (patch 0037). The two files of its module were missing from your master, patch 0039 adds them. r7 and L6: the mcchanges jar is found or downloaded by itself and deleted at the end (patch 0038). r15: archives in archives follow `bulk_scan_depth` (patch 0033). You marked them done
- [x] `maximum_width_and_height` is ignored, images of any size are accepted (patch 0036). The key is still accepted, so old options files work
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
- Do NOT waste usage limit/tokens and manage your resources efficiently
