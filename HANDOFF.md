# Handoff: the PackSquash fork (mocny-gaz/PackSquash-uv12)

This file records everything about the work that was added to this fork on top of
the original [PackSquash](https://github.com/ComunidadAylas/PackSquash). It is meant
for the next person or AI that continues the work, so that nothing has to be
guessed. Keep it up to date: update it in the same patch that changes the work.

- Fork base: original commit `4f0f501` ("fix(optifine): do not skip randomized model
  selection config for CEM"). All fork commits sit on top of it.
- Written by an AI (Claude) at the request of the fork owner, in a Linux sandbox. The
  owner works on Windows, with the project in `C:\apscl`, and keeps the mcchanges
  jars in `C:\amc`.
- Last update of this file: 2026-10-26.

## 1. Working agreements with the owner

- Work is delivered as `git format-patch` files, numbered `0001-...`, applied with
  `git am`. A patch is sent right after each finished implementation, without waiting
  for the next step.
- The order of work that the owner asked for: first the owner's own suggestions
  (done), then a first-release guide, then the upstream PackSquash features that are
  planned but not implemented, then a cleanup patch that makes the fork ready for its
  first release, then the "lite" mcchanges variant (see section 9).
- Long explanations can be text files instead of chat messages.
- The original README text must stay unchanged. The fork section is above it, and
  ends with a "Here's the original README:" line.
- Chat style the owner asked for: simple words, short sentences, active voice, no em
  dashes, no hashtags, no rhetorical questions, no filler.
- Names: the feature that was first called "baseline" is called **mcchanges** now.
  Options and files with the old names do not exist anymore.

## 2. Commit map

| # | Subject | What it contains |
| :-- | :--- | :--- |
| 1 | skip pack files identical to a baseline pack and unused OptiFine sky images | The first mcchanges feature (issue 402) and `skip_unused_optifine_sky_images`, plus `PackFileStatus::intentionally_skipped`. |
| 2 | remove language keys identical to a baseline pack and support versioned baseline packs | Language key removal (issue 197) and jars with many versions. |
| 3 | support Bedrock Edition resource packs | Issue 266. New `bedrock_pack` module and wiki page. |
| 4 | merge identical OptiFine sky images and document the fork | `merge_identical_optifine_sky_images`, README section, the tools folder. Clippy fixes. |
| 5 | rename baseline packs to mcchanges and add Bedrock Edition mcchanges jars | The rename, the rewritten jar builder for Java and Bedrock, ZIP64 support. |
| 6 | add the copy_as_is file option and support #include in shaders | Issues 338, 368 and 397. |
| 7 | store audio files of mcchanges jars as hashes | The `hashed` section and the `sha1` crate. |
| 8 | this handoff file and the README order | Documentation only. |

(Commit hashes change when the patches are applied by someone else, so they are not
listed.)

## 3. Features

### 3.1 mcchanges (original issue 402)

Option `mcchanges_path` (global). Points to a folder, a ZIP, or a client `.jar` with
vanilla files. A pack file whose path exists in it with equivalent contents is left
out of the result ZIP file. Code: `packages/packsquash/src/mcchanges.rs`.

How files are compared (`McChanges::is_redundant`):

- `.png`: same size and same RGBA pixels after decoding (spng, tRNS applied). The RGB
  of fully transparent pixels is ignored. Images above 4096x4096 pixels are compared
  byte by byte only.
- `.json`: parsed with `serde_json` and compared as values (key order and formatting
  do not matter). A UTF-8 BOM is ignored.
- Anything else: same bytes. Files known only by hash: same hash (see 3.3).
- A texture and its `.png.mcmeta` are left out together or kept together, because the
  game reads the animation metadata from the pack that provides the texture.
- Never left out: `pack.mcmeta`, `pack.png`, `manifest.json`, `pack_icon.png`,
  `bug_pack_icon.png`.
- When in doubt, the file is kept. Unreadable files, parse failures, and files that
  only one side has are never considered redundant.

The check runs in `PackSquasher::run` (`lib.rs`) before any file is processed.
Skipped files are reported as `skipped` with `intentionally_skipped` set, and the CLI
does not warn about them.

### 3.2 Versioned mcchanges files (`mcchanges_version`)

A jar or folder with a `mcchanges.json` at its root holds many game versions.
`mcchanges_version` is required for it, and an error for anything else. Format
(version 1):

```json
{
  "format": 1,
  "edition": "java",
  "versions": ["1.6.1", "1.7.2"],
  "removed": { "1.7.2": ["assets/minecraft/textures/blocks/old.png"] },
  "hashed": { "1.7.2": { "assets/minecraft/sounds/x.ogg": "sha1:0123..." } }
}
```

- `versions`: oldest to newest, in release order.
- `<version>/<path>`: the files that version added or changed compared with the
  version right before it. The oldest version has all its files.
- `removed`: files that version deleted compared with the version before it.
- `hashed`: files that version added or changed whose contents are not stored.
- Lookup (`McChanges::lookup`): from the selected version down to the oldest, the
  first version that has the file, has its hash, or removed it decides.
- `edition` is informational. PackSquash ignores unknown fields.

Archives are read by a small own ZIP reader (`mcchanges/zip_reader.rs`): stored and
deflated entries, ZIP64 (more than 65535 entries, and ZIP64 extra fields). It loads
the whole jar in memory.

### 3.3 Hashed files (audio)

`hashed` tokens are `sha1:<hex>` or `sha256:<hex>`. Audio files (`.ogg`, `.fsb`,
`.wav`, `.mp3`) are stored this way by the builder script to save space. For Java,
the SHA-1 comes from the Mojang asset index and the sound is never downloaded. For
Bedrock, the script computes SHA-256. PackSquash hashes the pack file and compares.
A re-encoded sound does not match. Crate: `sha1` (new dependency, one lock entry).

### 3.4 Language keys (original issue 197)

Option `skip_language_keys_identical_to_mcchanges` (global, default `true`, needs
`mcchanges_path`). `McChanges::reduce_language_file` removes entries of
`assets/<namespace>/lang/*.json` and `*.lang` whose value equals the vanilla value for
the same key and path. For `.lang` files the last value of a repeated key counts, and
every line of a removed key is removed. A file with no entries left is left out. The
reduced data replaces the file data through `file_data_override` in `lib.rs`
(`PackFileRead::Memory`), and the previous-ZIP reuse is disabled for such files.
Remaining JSON is written minified in the original order.

### 3.5 Bedrock Edition (original issue 266)

Option `pack_edition` = `auto` (default), `java`, `bedrock`. `auto` means Bedrock when
the pack folder has `manifest.json` and no `pack.mcmeta`/`pack.mcmetac`. Code:
`packages/packsquash/src/bedrock_pack.rs`, wiki page `docs/wiki/Bedrock-Edition-packs.md`.

- The manifest is validated (`validate_manifest`): format version, header name, uuid
  and version, modules with unique uuids. Problems that probably are mistakes give
  warnings (sent as `PackSquasherStatus::Notice`).
- Files are classified by `BedrockFileKind::of`: PNG (texture), JSON (comments allowed,
  key order kept, minified), OGG (existing Ogg Vorbis optimizer), everything else in a
  folder copied as is. Unknown files at the root are skipped, and can be included with
  `force_include`. `manifest.json` and `contents.json` are copied as is. `.lang` files
  are copied as is, because the Java language optimizer rejects valid Bedrock files.
- Textures use `skip_alpha_optimizations = true` by default, because Bedrock uses the
  alpha channel for other data in some textures (for example the S of MERS textures).
- Only `zip_spec_conformance_level = 'pedantic'` is accepted. Other levels were not
  checked against the Bedrock ZIP reader.
- Java quirks are cleared. Java only options do not apply.
- The existing machinery is reused through `PackFileAssetTypeMatches::of_asset_types`
  and the existing asset types `GenericTexture`, `GenericJsonWithComments`,
  `GenericOggVorbisAudio` and `Custom`.
- Never tested in the game.

### 3.6 OptiFine custom sky images

Code: `packages/packsquash/src/optifine_sky.rs`. Both options need `allow_mods` to
contain OptiFine, and only look at `assets/minecraft/optifine/sky/`.

- `skip_unused_optifine_sky_images`: a sky image is unused if no `.properties` file of
  the sky directory mentions its file name. Does nothing if there are no entries.
- `merge_identical_optifine_sky_images`: images with identical bytes (SHA-256) in the
  same directory are merged into the first one in path order. Sky entries of that
  directory that mention the removed image are rewritten (whole file names only).
  Images mentioned from another directory are not merged. The unused check runs on the
  rewritten text.

### 3.7 `copy_as_is` file option (issues 338 and 368)

`CustomFileOptions` has a new field `copy_as_is` (and `force_include` is optional now).
`copy_as_is = true` copies matching files untouched, even recognized ones, keeping their
path. Implemented as the first arm of `PackFileAssetTypeMatches::process_data`
(`pack_file/asset_type.rs`). `lib.rs` computes `copy_as_is` early so that language key
removal and sky entry rewriting leave those files alone. Tests: `copy_as_is_tests.rs`.

### 3.8 Shader `#include` (issue 397)

`pack_file/shader_file/parser.rs`: a regex (`INCLUDE_DIRECTIVE`) detects `#include`
at the start of a line. Such shaders are treated like an `#moj_import` that could not
be expanded: not transformed, and syntax errors are tentative (the shader is copied).
The field is still called `found_unresolvable_moj_import`.

### 3.9 Small changes

- `PackFileStatus::intentionally_skipped()`; the CLI warns only for other skips.
- `vfs.rs` has `read_to_end` and `read_file` helpers.
- The CLI crate is unchanged apart from the warning logic.

## 4. File map

The plain text guides for the owner are in `docs/fork/`: `FORK-EXPLAINED.txt` (what each
change does), `FIRST-RELEASE-GUIDE.txt` (git, build, release) and `MCCHANGES-JAR-GUIDE.txt`
(building and using the jars).

| Path | Purpose |
| :--- | :--- |
| `packages/packsquash/src/mcchanges.rs` | mcchanges loading, comparison, language reduction. |
| `packages/packsquash/src/mcchanges/zip_reader.rs` | Minimal ZIP/ZIP64 reader. |
| `packages/packsquash/src/mcchanges/tests.rs` | Tests, with helpers `png`, `zip`, `temp_dir`, `write_file` used by other test modules. |
| `packages/packsquash/src/bedrock_pack.rs` (+ `tests.rs`) | Bedrock manifest validation, classification, defaults. |
| `packages/packsquash/src/optifine_sky.rs` (+ `tests.rs`) | Sky image optimizer. |
| `packages/packsquash/src/copy_as_is_tests.rs` | Tests of `copy_as_is`. |
| `packages/packsquash/src/lib.rs` | Wiring in `PackSquasher::run` and the per-file task. |
| `packages/packsquash/src/config/mod.rs` | New options and the `PackEdition` enum. |
| `tools/mcchanges-jar/` | `build_mcchanges_jar.py` and the AI prompt. |
| `docs/wiki/Options-files.md` | Option documentation. |
| `docs/wiki/Bedrock-Edition-packs.md` | Bedrock page. |
| `CHANGELOG.md` | Everything is under "Unreleased". |

## 5. The jar builder (`tools/mcchanges-jar/build_mcchanges_jar.py`)

Python 3, standard library only. Subcommands: `java`, `bedrock`, `pack` (offline).

```
python tools\mcchanges-jar\build_mcchanges_jar.py java --output C:\amc\mcchanges-java.jar
python tools\mcchanges-jar\build_mcchanges_jar.py java --output C:\amc\mcchanges-java.jar --update
python tools\mcchanges-jar\build_mcchanges_jar.py bedrock --output C:\amc\mcchanges-bedrock.jar --count 5
```

- Java: Mojang manifest, releases from `--first` (default 1.6.1), client jar plus
  language files and `sounds.json` from the asset index, sounds as SHA-1 hashes.
- Bedrock: tags of `Mojang/bedrock-samples` (without previews), `resource_pack` folder.
  Tags are listed with `git ls-remote`, with the GitHub API as fallback. Each download
  is about 150 MB.
- `--audio hash|none|full`. `--update` appends versions and gives the same file as a
  full rebuild. Every build is verified by rebuilding each version from the output.
- Work files stay in `mcchanges-work`, so an interrupted run continues.
- Tested: against a local mock of the Mojang servers, and with real Bedrock releases.
  Not tested: the real Mojang servers (the sandbox could not reach them).
- The jar contains Mojang files. Do not publish it. The README and the guides say so.

## 6. Build, test and lint

```
cargo build --release -p packsquash_cli          # target\release\packsquash.exe
cargo test -p packsquash --lib
cargo fmt --all
cargo clippy --no-deps --all-targets -- -D warnings
```

Notes that cost time before:

- `rust-toolchain` says `nightly`. The project needs nightly features. In the sandbox
  only stable was available, and it was used with `RUSTC_BOOTSTRAP=1`.
- **Formatting was never run with the real toolchain.** The project's `rustfmt.toml`
  uses `trailing_comma = "Never"`, which stable rustfmt ignores (it reformats the whole
  tree with trailing commas). The code was formatted by hand in the project style. Run
  `cargo fmt --all` with nightly before the first release and commit the result.
- Clippy was clean with stable 1.99 for every patch (`--no-deps --all-targets`).
- Test state at the last check: 153 pass, 1 fails
  (`squash_zip::system_id::tests::get_aggregated_dmi_serial_numbers_id`, it needs real
  hardware serial numbers and is not related to the fork). Two `ditherbomb` PNG tests
  were skipped because they are very slow in a debug build.
- Nothing was compiled or run on Windows.

## 7. CI and release facts

See `docs/fork/FIRST-RELEASE-GUIDE.txt` for the step by step version. The facts behind it:

- `.github/workflows/ci.yml` runs on pushes to `master` and on tags `v*`. A tag run
  builds Windows, Linux (glibc and musl, x86-64 and ARM) and macOS programs, a Debian
  package and a Docker image, and creates a **draft** GitHub release.
- Three jobs use the paid runner label `blacksmith-4vcpu-ubuntu-2404`. A fork must
  replace it with `ubuntu-24.04`.
- Secret `CONST_RANDOM_SEED` (any random text) seeds string obfuscation.
- The `benchmark` job needs a private password and fails without it (not critical).
- `.github/workflows/wiki-sync.yml` syncs `docs/wiki` to a GitHub wiki that the fork
  does not have. Remove it.
- ARM runners are free for public repositories only.
- Workspace version is `0.4.1` in the root `Cargo.toml`. Suggested first version:
  `0.5.0-fork.1`.

## 8. Upstream issue status (checked 2026-10-26)

| Issue | State |
| :--- | :--- |
| 402 texture debloat, 197 language keys, 266 Bedrock | Done in this fork. |
| 397 `#include` in shaders | Done (fallback, not a full resolver). |
| 338, 368 ignore files | Done as the `copy_as_is` file option, not as a `.packsquashignore` file. |
| 254 pack overlays, 373 new pack formats since 1.21.9 | Already in the code of the original `master` (overlay layers and `min_format`/`max_format` parsing, with tests). Not released upstream yet. |
| 390 optimizations for `force_include`d files | The maintainer argues against it. Not done. |
| 35 GUI, 196 Gradle plugin, 238 Bukkit plugin, 378 Java bindings, 215 API crate | Separate products or large projects. Not done. |
| 119 more shader minification, 362 function inlining, 300 Nemo's Transit Expansion | Not done. Need study. |

## 9. Open work

1. **Upstream backlog**: see section 8.
2. **Cleanup and release preparation patch**: apply the edits of `docs/fork/FIRST-RELEASE-GUIDE.txt`
   (runner labels, remove the wiki sync, version, changelog heading), fix the odd test
   name `archives_can_be_mcchangess` in `mcchanges/tests.rs` (left from the rename), and run
   nightly `cargo fmt`.
3. **mcchanges-lite**: a second jar kind that stores no files at all, only hashes. Planned
   design:
   - The index lists, for each version, a token per file: `sha256:` of the bytes for
     most files, `png:` plus size and SHA-256 of the normalized RGBA pixels for textures
     (same normalization as `decode_png` in `mcchanges.rs`, including zeroing the RGB of
     transparent pixels), `json:` plus the SHA-256 of canonical JSON (keys sorted, so
     order does not matter) for JSON files, and per key hashes for language files.
   - The builder needs a PNG decoder in pure Python. PNGs it cannot decode exactly
     (16 bit, interlaced) fall back to a byte hash.
   - The Python tokens must be cross-checked against the Rust ones on real data (the
     5000 PNGs of a Bedrock release are a good set).
   - PackSquash needs `McChangesEntry` variants for these tokens, a texture check that
     compares tokens, and a language reduction that compares per key hashes.
   - Files in `C:\amc`: `mcchanges-java.jar`, `mcchanges-bedrock.jar` (normal),
     `mcchanges-lite-java.jar`, `mcchanges-lite-bedrock.jar` (lite). A lite file holds no
     Mojang files, which may make sharing it less problematic. That is not a legal
     opinion: check Mojang's rules.
4. First release (the owner does it with the guide).
5. Ideas not started: a `.packsquashignore` file, a hash-only mode for the builder
   `--update` without keeping old files.

## 10. Lessons and gotchas

- `git am --reset-author` does not exist. To change the author after applying patches:
  `git rebase master --exec "git commit --amend --reset-author --no-edit"`.
- Real data finds bugs that synthetic tests do not: an empty-path entry from a ZIP
  directory prefix broke the Bedrock jar builder until it was tested on a real release.
- The Bedrock samples have about 4800 `.fsb` sound banks and 13000 other files. The jar
  of two releases is about 82 MB.
- GitHub API calls from the sandbox answered HTTP 403, `git ls-remote` worked.
- `web_fetch` can only open URLs that appeared before in search results or pages.
- Do not use `cargo fmt` from a stable toolchain on this repository.
- A hand-built test archive must have exact field lengths. The ZIP64 test archive needed
  two fixes for that.

## 11. How to continue (checklist for the next session)

1. `git log --oneline 4f0f501..HEAD` and read this file and `CHANGELOG.md`.
2. Run the tests (section 6), expecting the known failure.
3. Do one item of section 9, write tests with it, run clippy.
4. Update this file, `CHANGELOG.md` and the wiki pages in the same patch.
5. Export the patch with `git format-patch -1 -M --stdout > NNNN-name.patch` and check
   that the whole series applies to `4f0f501` and gives the same tree.
