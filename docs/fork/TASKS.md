# Task list of the fork

Last update: 2026-10-30. The owner sets the order. A task is done when its patch is applied and pushed.
Tools (jar builder, release script, apply script) are NOT in this repository. They live in `packsquash-tools`.

## Now

| # | Task | Status | Who |
|---|---|---|---|
| N1 | Apply patch 0011 (release prep: version 0.5.0-fork.1, ubuntu-24.04 runners, no wiki sync, changelog heading) | patch sent | owner |
| N2 | Dependabot alerts (5 on 2026-10-29). Found 4: `tokio` 1.54.0, `libz-sys` 1.1.30, `ctutils` 0.4.3, `certifi` 2025.11.12 (test packs). The 5th needs the alert list | patch 0013 | owner pastes the 5th alert |
| N3 | Compact mcchanges format: language key lists are 98% of the 81 MB jar. New tokens `langfull:` and `langdelta:` (8 byte hashes, changes only) | patch 0014 + `build_mcchanges_compact.py` | AI |
| N4 | First release: `python release.py release` (see `03-release-script/README.md`) | waits for N1, N2 | owner |
| N5 | Publish the jars in the `mc-changes` repository as GitHub Release files (not in git, not LFS). README for it | README sent | owner |
| N6 | Run nightly `cargo fmt --all` once, commit | not done | owner |

## PackSquash features asked by the owner (after everything above)

| # | Feature | Notes |
|---|---|---|
| F1 | **Any archive format as input.** `.zip` today. Add `.7z`, `.tar`, `.tar.gz`, `.tar.xz`, `.tar.zst`, `.rar` (read only) and others, as long as the contents look like a pack (`pack.mcmeta` with `assets/`, or a Bedrock `manifest.json`) | The pack can be in a subfolder of the archive. Detect by contents, not by the extension. Output stays a zip |
| F2 | **Bulk squash.** Squash every pack found in a set directory, however the directory looks, as long as somewhere there is a folder or an archive whose contents match a pack | Default: the output goes next to the input, with the prefix `squashed_`. Options: custom prefix, custom suffix, and an option to overwrite (replace) the original pack with the optimised one. Needs a dry run, a report, and care with overwriting (keep a backup unless told not to) |
| F3 | **Very simple GUI**, like [Win11Debloat](https://github.com/Raphire/Win11Debloat) but only with a dark OLED (pure black) mode. Easy configuration, and export (and import) of the configuration | LATER RELEASE. Lowest priority. Everything else comes first |

## Later (upstream issues and other ideas)

| # | Task | Status |
|---|---|---|
| L1 | Issue 119: shader minification | not started, needs study |
| L2 | Issue 362: function inlining | not started, needs study |
| L3 | Issue 300: Nemo's Transit Expansion | not started, needs study |
| L4 | Upstream issues 254 (overlays) and 373 (formats): in upstream master, not released | wait for upstream |
| L5 | Large separate projects, not planned: 35 (GUI, see F3), 196 (Gradle), 238 (Bukkit), 378 (Java bindings), 215 (API crate) | not planned |
| L6 | `mcchanges_download` option (download the jar with curl, keep the old file offline) | never built, the owner did not ask again |
| L7 | Linux and macOS programs in the release (the local script only makes the Windows one; the CI way makes all) | optional |
| L8 | Java data was not cross-checked between the builder and PackSquash (Mojang servers were not reachable from the AI sandbox) | do it with the owner's jar |

## Done

| Patch | What |
|---|---|
| 0001 to 0008 | skip files that are the same as vanilla (mcchanges), versioned mcchanges, Bedrock, language keys, sky images, `copy_as_is`, `#include` in shaders, audio as hashes, handoff and docs |
| 0010 (in master as "mc-changes ready") | hash tokens in mcchanges files: `png:`, `json:`, `langkeys:` |
| 0011 | release prep |

## Rules

- Scripts and their READMEs are separate files, never in a patch.
- Send each patch or file right after it is finished, then go on with this list.
- Simple English. Windows 11 and cmd commands.
