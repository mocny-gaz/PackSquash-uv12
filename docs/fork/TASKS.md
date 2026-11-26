# Task list of the fork

Updated 2026-11-26. Edit it freely and upload it. One line per task: `- [ ] ID text (who)`. Put long text, like an error, in a
block under the task. Fixed things move to Done, in one short line. Tasks are sorted by ID. STATE is what the AI did.

## Bugs

- [ ] b9 `Temmie.zip`: one `.lang` file with a line without `=` fails the whole pack (ai) STATE: patch 0046 (in your master). The file is copied as it is. Tested on a real pack
- [ ] b10 12 of 46 Modrinth packs fail because of the checks of `pack.mcmeta` (`supported_formats` with `min_format`, three numbers in a version, a decimal number) (ai) STATE: patch 0047 (in your master). The pack is squashed anyway, with a notice, when `copy_failed_files` is on. Tested with the exact error of the log
- [ ] b11 when typing in the settings file, if you paste the indicator of where youre typing is duplicated, also instead make it just like in ms notepad app (ai) STATE: patch 0057 (in your master). Cause: every key press resized the editor, so FLTK drew the old caret again. It is resized only when its place changes now. The caret is a thin line that blinks like the one of Notepad. Ctrl+Backspace and Ctrl+Delete remove a word, the right button has a menu (Undo, Cut, Copy, Paste, Delete, Select All), and the page shows Ln and Col. Ctrl+Z, Ctrl+Y, Ctrl+A, Ctrl+arrows and Tab worked already. Tested on a virtual screen with a real paste of several lines
- [ ] b12 the packsquash console in background has a weird name with some chinese letters and path to some winget and other bs (ai) STATE: patch 0057 (in your master). Cause: the title was given to Windows without the zero that ends it, so Windows read the memory after it: garbage with Chinese letters and a WinGet path. It ends with a zero now. The title is `PackSquash-uv12 - Optimizing [-] | CPU 37% | pack: NAME`. Not tested on Windows. The Windows code compiles for the Windows target with `-D unsafe-code`

## This very moment

/// priority is b12, r17, f11, r18, and the rest by default priority
- [ ] r16 remake allow_mods to be instead optimise_mods (because anything in /assets that is like /assets/modname is another mod and by default theyre normally optimised, so instead make it so that for example assets/mcpatcher optimises optifine same for assets/optifine, add any other popular mod support(by that i mean that specialised like the sky debloat in optifine) and also make optifine optimisation settings be gray(to show taht unavailable) and skipped if it isnt in optimise_mods) (ai) STATE: design answered, after f12. `optimise_mods` with `auto`, `allow_mods` works with a warning, OptiFine stays as it is. Checked: the folders `assets/minecraft/optifine` and `assets/minecraft/mcpatcher` are right
- [ ] r17 add back the colors to console in background (ai) STATE: patch 0057 (in your master). The window starts PackSquash with `--color`, the terminal shows its colors, and the Windows console is told to understand them. The browser page stays plain text. Tested: the colors arrive in the output. Not tested on a Windows console
- [ ] r18 after making f11 add pararel mode (only accesible when schedule) you can set how many schedules in pararel (it will split the cpu threads evenly, and not pack each on each) 0 is off (ai) STATE: patch 0059 (sent). `parallel = N` in the `[schedule]` table runs N parts at the same time, each as its own run, with output lines that start with `[part N]`. A part that ends gives its slot to the next one. A new part gets the free threads divided by the parts that run beside it, and never more than its own `threads`. When the last parts run, a bulk part takes the free threads from its next pack (a single pack keeps its threads). The same output in two parallel parts is an error, the same input is a warning. Tested: parallel runs with a failing part, the live thread update on a real bulk run, the split by numbers. Not tested: a computer with more than one CPU thread (the sandbox has one) and Windows

## Features (in progress: the AI prepares the patches, the owner builds, then applies)

- [ ] f6 a fixer which will when fixer = true fix common detected issues in resourcepack *only when a pack/file failed* (ai) STATE: ON HOLD until you say. The fixer, the Bedrock conversion and the version conversion wait. The JSON fixer is built (patches 0043 and 0044, kept ready, not applied)
- [ ] f11 add squash scheduling, make a button at topbar with add schedule after being clicked in topbar create schedule 1 and a + for another one and so on. each schedule has its own settings and when exporting you can export or one or whole schedule (yes add it as not only gui function but as in .toml too and cli of course) and when run will run one by one, allow editing schedule untill that part of it is running, before running another part of schedule, check if any settings in that part have been changed in past 10 seconds, if not then wait 3 seconds and run it, if yes wait till full 10 seconds pass from last modifications to that part of schedule and then run. in bottombar add functions(which will appear when schedule mode is enabled) such as stop schedule, stop schedule part(of the one currently in), skip schedule part(will go to next one and come back after all are done ), delete schedule part (will remove the part). also add consistent schedule numbering in topbar (ai) STATE: patch 0058 (sent) has the schedule file and the terminal. `packsquash schedule.toml` runs the parts one by one. The file has a `[schedule]` table anywhere and a `[[part]]` line before the options of each part, with no names or numbers (`[[part2]]` is an error). Every problem is shown at once. A failed part is told when it fails and again at the end, and the schedule goes on. Left: the window (tabs, the + button, Pause, Stop part, Stop schedule and Delete part at the bottom, the 10 and 3 second wait, export of one part or the schedule, old numbers kept after Delete). No Skip part, as you answered
- [ ] f12 pause button (make it NOT bug/corrupt) (ai) STATE: design answered, after r18. The window only, waits for the current files, a running download restarts with a warning, high iteration options get a warning
- [ ] f13 in background console to the header(the name the one i also reference in b12) cpu % used by PACKSQUASH (as the current window/task) and which bulk/input its squashing (not the file to make it not spam at all) (ai) STATE: patch 0057 (in your master). The console title shows the CPU use of PackSquash and the pack, or `bulk: NAME` in the bulk mode, never a file. It refreshes twice a second. Tested on Linux with a terminal: `PackSquash-uv12 - Optimizing [-] | CPU 99% | pack: bigpack`. The Windows CPU code compiles for the Windows target. Not tested on Windows

## Later

- [ ] L0 make packsquash-uv12 not be some fork in release nor tag but packsquash-uv12 1.0 adding +0.1 each time, make it go 1.10 change the 2.0 only when user requests( also the gui update should make it 2.0 already and +0.1 for each more update but bigger one and not every commit) (ai) STATE: answered, last. Tags and titles look like `UV12-0.2.1`, with Cargo `0.2.1`. The first window release is 0.2.1
- [ ] L1 Issue 119: shader minification. Needs study STATE: study note sent. The first priority tasks are done, so it starts now, but I have no shader data here (Mojang is not reachable). Send a pack with large shaders (`assets/minecraft/shaders`), then I measure the real gain first
- [ ] L2 Issue 362: function inlining. Needs study STATE: study note sent. Needs a data pack with many `.mcfunction` files to test on
- [ ] L3 Issue 300: Nemo's Transit Expansion. Needs study STATE: study note sent. Already works in the fork: `.obj`, `.mtl` and `.js` under `assets/mtr` are kept (tested, with and without the MTR option). Please test one real NTE pack, I have none here
- [ ] L4 Upstream issues 254 (overlays) and 373 (formats): wait for upstream
- [ ] L5 Not planned: 196 (Gradle), 238 (Bukkit), 378 (Java bindings), 215 (API crate)
- [ ] L7 Linux and macOS programs in the release: the CI way (`release.py tag`) makes them (ai) STATE: checked. The CI of a tag push builds Linux (x86_64 and aarch64, gnu and musl), Windows and macOS (universal2). Nothing to do, you can close it
- [ ] L8 Cross-check Java tokens between the builder and PackSquash with real Java data (Mojang was not reachable from the AI sandbox)
- [ ] L9 when f3 gui will be made at around that part of development and when f6 be implemented make a mode at the bottom of gui to just FIX if any issues with packs aand it will just check everything and then a prompt will appear to apply changes (bypassable with fixer_prompt = false)
- [ ] L10 find out which mods you use most, so they get special support first (ai)

## Done

- [x] The debug log (`log = true`) tells now how the run started (terminal, or the program window with its page, size, screen, hand edited text, changed options, warnings and the last actions), where the log is and why, and the options of PackSquash with all defaults filled in (patch 0057)
- [x] F3* (the window): patch 0042 was a browser page and was wrong. Patch 0055 is a real program window made by FLTK inside `packsquash.exe`, no browser, only Windows system DLLs. Patch 0056: PackSquash-uv12 in the top left, 2x faster wheel, no Log page, the Settings file page can be edited by hand and takes a paste. `packsquash --gui options.toml` loads a file. Linux and macOS keep the browser page. Not tested on a real Windows screen
- [x] r1*: the option `fix_pack_metadata`, default on, repairs a `pack.mcmeta` that fails the checks and shows each change as a notice (patch 0053)
- [x] old b12: the tester build failed with "usage of an `unsafe` block" in `gui.rs`. Patch 0052

- [x] The 9 tests that failed in your `tests.txt` (also `a_blank_game_version_compares_with_every_version_and_auto_chooses_one`) all failed with "Unexpected number of strong references to the panic hook". The `cargo test` threads changed the global panic hook at the same time. They take turns now, and the program is unchanged (patch 0056). The debug log (`log = true`) is written where the output is, not next to the options file (patch 0056)
- [x] A test still expected the old option name `skip_optifine_sky_entries_with_missing_images` and failed, which would have failed the CI of a tag push (patch 0054)
- [x] b8 and the old b11: not bugs. Patch 0048 is reverted (patch 0049). r6: design file sent. r17, r18, r20: release script (normal release, window test, free tags). f7: results are an archive of the same kind as the input (patch 0035)
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
- Simple English. Windows 10 and cmd commands.
- Clean up after user and sort and order each time
- delete comments(//) on resend. if answer is asked just make another comment and delete old one. if /// then delete only after all from there is done
- give patches as soon as theyre done and continue moving with the task list, if theres sth which needs users info or choice then ask it and go on with the list not wasting resources
- obey .rules but prioritise user
- prioritise user sent TASKS.md over this file
- Do NOT waste usage limit/tokens and manage your resources efficiently
- when * means that it could be changed/is temporary
- When a task is done and sent, go on with the next one at once. Never stop and wait.
- priority: user custom in chat + /// + //, b, r, f, l   (the + is also in priority order sorted)
