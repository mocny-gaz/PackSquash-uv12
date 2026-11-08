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
- Last update of this file: 2026-11-04.

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
`mcchanges_version` selects the version, and it is an error for anything else. If it
is not set, empty or `auto`, `McChanges::open_auto` chooses the version (see 3.12). Format
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

### 3.10 Archive input (F1)

- Library module `pack_archive.rs` (+ `pack_archive/tests.rs`): `detect_format` looks at the first
  bytes (ZIP, 7z, RAR, gzip, xz, Zstandard, bzip2, TAR by the `ustar` magic). `scan_archive` lists
  the entries and finds pack roots: a directory with `pack.mcmeta` and `assets/` or `data/`, or a
  Bedrock `manifest.json` with a resources module. `extract_packs` extracts only the packs into a
  temporary directory (`ExtractedPacks`, deleted on drop). Names with `..`, absolute paths, drive
  letters and backslash tricks are rejected, links and special entries are skipped, and there are
  limits for bytes and entries.
- Pure Rust readers: `zip`, `sevenz-rust2`, `tar`, `bzip2` (libbz2-rs-sys), `lzma-rust2` (xz) and
  `ruzstd`. RAR is the only `ArchiveFormat::Helper` case: `extract_with_helper` runs `bsdtar`, `tar`,
  `7z`, `7zz`, `7za` or `unrar`, then `clean_helper_output` deletes links and checks the limits.
- CLI: `fork_options.rs` reads and removes `archive_pack_search_depth` and `archive_max_extracted_mib`
  from the options table before `SquashOptions` is read (without them, the text is parsed directly,
  so error messages keep their line numbers). `prepare_archive_input` in `main.rs` runs when
  `pack_directory` is a file. Exit code 4 is an archive error.
- Tested: 10 unit tests of the module, 5 of the CLI options, and an end to end script of 34 runs
  (every format, no extension, subfolder, depth, two packs, no pack, limit, traversal in a TAR, a fake
  `bsdtar` that makes a link). Not tested: real RAR files, Windows `tar.exe`.

### 3.11 Bulk mode (F2)

- CLI only: `bulk.rs` (+ `bulk/tests.rs`) and the `bulk_*` options parsed in `fork_options.rs`
  (`BulkOptions`, removed from the table before `SquashOptions` is read; the table is kept, and every
  pack gets its own `SquashOptions` from a clone with `pack_directory` and `output_file_path` set).
  `bulk::run` gets the squash function as a parameter, so the tests use a fake one.
- `discover` walks the tree without following links. A pack directory is not searched inside. Files
  are opened as archives only if their extension is in `bulk_archive_extensions` (`jar` is not), then
  `scan_archive` decides by contents. Names that look like earlier results (prefix and suffix) are skipped.
- Archives are extracted once with `extract_packs`, all their packs are squashed, and the temporary
  directory is deleted. Several packs in one archive: `PREFIX + STEM + _ + FOLDER + SUFFIX + .zip`.
- Safety: `Guard` refuses a result that is an input, a directory, or already given to another pack.
  `bulk_overwrite` writes `STEM.zip.squashing`, checks that `STEM.zip` is the original or does not
  exist, moves the original to `NAME.ext.original` (refuses if it exists), then renames. If the last
  rename fails, the original is put back. A directory is never replaced. A failed squash removes the
  temporary file and keeps the original.
- `bulk_scan_depth`: a pack or archive directly in the input directory is level 1.
- Exit codes: 1 if any pack failed, 4 for a bad input directory or an archive error, 3 for bad options.
- Tested: 17 unit tests of `bulk.rs`, 6 of the options, and an end to end script of 13 runs with the
  built program (dry run, normal, second run, prefix and suffix, overwrite with and without backup,
  a bad pack, option errors, file option tables). Not tested: Windows paths and long paths.

### 3.12 Automatic game version (f4)

- `McChanges::open_auto` (in `mcchanges.rs`) is used by `lib.rs` when `mcchanges_version` is not set,
  empty (after trim) or `auto` (any case). `open` is still used for an explicit version. A path that is
  not versioned is used as it is for a blank version, and is an error (`NotVersioned`) for `auto`.
- `detect_version` counts identical files per version without comparing every file with every version.
  A path only changes in the versions that mention it (a data file `VERSION/PATH`, a `hashed` token, or a
  `removed` entry), so each pack file is compared once per mentioning version, and the result counts for
  every version until the next mention (a difference array). Skipped: `pack.mcmeta` and the other
  pack identity files (`is_pack_identity_file`), anything with `/lang/` in its path, and language tokens.
  The comparison is the one of `is_redundant` (PNG pixels, canonical JSON, bytes, tokens), without the
  texture and `.mcmeta` pairing.
- Ties go to the newest version. With no identical file at all, the newest version is used and the
  notice says that no file is left out (the first plan was an error; this cannot drop a needed file).
- `VersionDetection::describe` makes the notice, which `lib.rs` sends as `PackSquasherStatus::Notice`.
- Tested: 8 unit tests (`mcchanges/tests.rs`: best version, tie, no match, hashes, directory,
  not versioned, empty index) plus a squash test, and an end to end script with the built program and
  the real `mcchanges-java.jar`. With 8000 pack files and the real 87 version jar, a debug build took
  2.2 s for the choice and 1.4 s to check the same files against one given version.

### 3.13 OptiFine sky entries with a missing image (r2)

- `optifine_sky.rs`: `SkyImageOptimizer::scan` has a third flag, `skip_missing_image_entries`, from the global
  option `skip_optifine_sky_entries_with_missing_images`. It collects all pack paths, reads the `source` of each
  entry like a Java properties file (`source_property`: last one wins, comments, `=` `:` or space; a line that ends with a
  backslash makes the entry unclear, so it is kept), and resolves it like OptiFine does (`source_image_path`, from the
  official `sky.properties` doc: `path` is relative to assets/minecraft, `~/` to assets/minecraft/optifine, `./` to the
  entry, `ns:path` to the namespace). No source means `sky<n>.png` next to a `sky<n>.properties` entry.
- Only an image under `assets/minecraft/optifine/` that is not in the pack counts as missing, because the game and other
  packs can provide the rest. Unclear paths, `..`, other namespaces and `/` paths are kept.
- OptiFine stops loading `sky<n>.properties` at the first missing number, so entries are checked from the highest number
  down, and an entry is only left out if every higher numbered entry is left out too. `sun.properties`,
  `moon_phases.properties` and other names are not in the sequence.
- The result is `skipped_entries` (reported as intentionally skipped files, with the reason "OptiFine custom sky entry whose
  image is missing") and `notices()`, which `lib.rs` sends as notices, with a warning about unfinished packs.
- Not done: other OptiFine properties (CTM `tiles`, CIT `texture`/`model`, random entities, and so on). The owner wrote
  "sky models and others"; the same approach applies, one resolver per file type.
- Tested: 9 new unit tests and a squash test; an end to end run with the CLI.

### 3.14 A folder as mcchanges_path (r4)

- `find_mcchanges_jar_in_directory(vfs, path, is_bedrock)` in `mcchanges.rs`, called by `lib.rs` before `open` and `open_auto`.
  It only acts if `path` is a directory with no `mcchanges.json` at its root and it has a file with the exact name
  `mcchanges-java.jar` (Java pack) or `mcchanges-bedrock.jar` (Bedrock pack) that is a ZIP file with a `mcchanges.json`
  inside. It returns that jar and a notice. No other name is looked for (the owner asked for no complexity).
- Tested: 2 unit tests, and a run with the CLI and the real jar in a folder.

### 3.15 Keep unrecognized asset files (r1 and r9)

- Global option `include_unrecognized_asset_files` (`config/mod.rs`). In `lib.rs`, right before the "skipped" result of a
  pack file, a file whose path is `assets/<namespace>/...` is processed with a custom asset match and
  `CustomFileOptions { force_include: true }`, like a file that the user force included (copied as it is). Not for Bedrock.
- The owner asked for `assets/minecraft` (r1), then for any folder of `assets` (r9). `data` is not covered (he did not ask).
- Some extensions are already kept by default in other namespaces (a `.bin` file was kept, a `.dat` file in a known directory
  was skipped), so tests use `.xyz`. A `data/` file in a test pack makes `AmbiguousPackType`.
- Tested: `copy_as_is_tests::unrecognized_asset_files_are_kept_only_if_asked`.

### 3.16 An archive file as bulk input (r8)

- `bulk::run` accepts a file as `bulk_input_directory`: `discover_archive_file` scans it (by contents, not by extension) and
  makes one `Found::Archive`. Not an archive is exit code 4; an archive without a pack is like an empty directory (code 0).
  A path that does not exist says so and mentions both kinds of input. Its name is not checked for looking like a result.
- Tested: 2 unit tests, and a CLI run with a real 7z and a ZIP with the pack in a subfolder.

### 3.17 The debug log (r5 and r10)

- Library: `debug_log.rs` has the log target `debuglog` (`DEBUG_LOG_TARGET`), the `detail!` and `detail_error!` macros (they
  only evaluate their arguments if a logger wants the target) and `error_chain` (`describe_error_chain`: every error with its
  causes and its Debug text). The library depends on `log` now. Messages are sent from `lib.rs` (pack edition, quirks, asset
  type mask, layers, every pack file, asset type matches, file options tried, skip reasons, error chains where errors become
  status messages, copied from the previous run), `mcchanges.rs` (kind of pack, selected version, every lookup with the layer
  and the token, `describe_hash_comparison` and `describe_data_comparison` with the pack file's own token, texture and
  language decisions, the counts of the automatic choice), `optifine_sky.rs` and `pack_archive.rs` (first bytes, entries,
  unsafe names, links, helper program command, exit status and output).
- CLI: `debug_log.rs` has `TeeLogger`, which sits in front of the `env_logger` console logger. The console shows what it showed
  (the console filter is the `packsquash` target at every level; `debuglog` is not shown). The file gets every message of the
  `packsquash*` targets, the `debuglog` target, and warnings of other crates. Lines logged before the options are read are kept
  (at most 500, what the console shows) and written first. `start` is called from `start_debug_log` in `main.rs` after the options
  are parsed (`log` and `log_file` are CLI options, in `fork_options.rs`). The header, the options file text, status results,
  the error chain, the panic hook (`log_panics`) and the exit code are written there.
- The file is `packsquash-debug.log` next to the options file by default, replaced each run. Warnings and errors are flushed at
  once, the rest when the buffer is full and at the end.
- Tested: 3 library tests, 6 CLI tests (time format, line format, options), end to end runs with broken Ogg, PNG and ZIP files
  (the Vorbis error shows its library, function and kind).
- r12 and r8: the bulk mode now uses `max(bulk_scan_depth, archive_pack_search_depth)` as the depth inside archives. The owner's
  "204" cap could not be reproduced with 300 packs in a 7z, a ZIP or a folder, so it is probably this depth, or data specific:
  the bulk search is logged ("not searched, because it is deeper than bulk_scan_depth", "Bulk search done: N packs").

### 3.18 Compare with every version (r13)

- `McChanges::open_any_version` (blank or unset `mcchanges_version`) keeps the layers of the newest version and sets
  `any_version`. `is_redundant`, texture pairs and language reduction then run once per state of the path: `defining_layers`
  gives the layers that mention a path (a data file, a hash token or a deletion), `comparison_positions` adds the
  `.mcmeta` of a texture, and `lookup_from(path, start)` gives the file as it was in the version of that layer.
  Language keys are removed in turn for every state (a key goes if it has the value in any version).
- `auto` still uses `open_auto` (one best version, f4). Not versioned: blank is used as it is, `auto` is an error.
- Risk, told in the docs and in a notice: a file identical to an old version only is left out.
- Tested: 2 new unit tests and the updated squash test.

## 4. File map

The user guide is `docs/fork/USAGE.md`. The plain text guides for the owner are in
`docs/fork/`: `FORK-EXPLAINED.txt` (what each change does), `FIRST-RELEASE-GUIDE.txt` (git,
build, release) and `MCCHANGES-JAR-GUIDE.txt` (where the jars come from, short).

| Path | Purpose |
| :--- | :--- |
| `packages/packsquash/src/mcchanges.rs` | mcchanges loading, comparison, language reduction. |
| `packages/packsquash/src/mcchanges/zip_reader.rs` | Minimal ZIP/ZIP64 reader. |
| `packages/packsquash/src/mcchanges/tests.rs` | Tests, with helpers `png`, `zip`, `temp_dir`, `write_file` used by other test modules. |
| `packages/packsquash/src/bedrock_pack.rs` (+ `tests.rs`) | Bedrock manifest validation, classification, defaults. |
| `packages/packsquash/src/optifine_sky.rs` (+ `tests.rs`) | Sky image optimizer. |
| `packages/packsquash/src/copy_as_is_tests.rs` | Tests of `copy_as_is`. |
| `packages/packsquash/src/pack_archive.rs` (+ `pack_archive/tests.rs`) | Archive detection, pack search and safe extraction. |
| `packages/packsquash_cli/src/bin/packsquash/fork_options.rs` | Options that only the CLI reads (archive limits). |
| `packages/packsquash_cli/src/bin/packsquash/bulk.rs` (+ `bulk/tests.rs`) | Bulk mode: search, names, safe replacement, report. |
| `packages/packsquash_cli/src/bin/packsquash/error_hints.rs` | The `How to fix` lines printed after an error. |
| `packages/packsquash/src/lib.rs` | Wiring in `PackSquasher::run` and the per-file task. |
| `packages/packsquash/src/config/mod.rs` | New options and the `PackEdition` enum. |
| `docs/fork/USAGE.md` | User guide: quick start, defaults, mcchanges, problems. |
| `docs/wiki/Options-files.md` | Option documentation. |
| `docs/wiki/Bedrock-Edition-packs.md` | Bedrock page. |
| `CHANGELOG.md` | Everything is under "Unreleased". |

## 5. The jar builder (outside this repository)

The scripts are not part of PackSquash and are not in this repository. The owner keeps
them in `C:\amc\packsquash-tools\`:

| Folder | What |
| :--- | :--- |
| `01-mcchanges-builder` | `build_mcchanges_jar.py` (the builder), its tests, README and AI hand-off. |
| `03-release-script` | `release.py`: builds and publishes a release, following the checklist in `docs/wiki/New-release-checklist-(for-maintainers).md`. |
| `04-apply-patches` | `apply_patches.py`: applies patch files to this repository. |

A copy of the builder, the format description and the finished jars are in the repository
[mc-changes](https://github.com/mocny-gaz/mc-changes). The jars are Release files there.

- Commands of the builder: `java`, `bedrock`, `pack` (offline) and `info`. Everything is
  written in `C:\amc` (jars and the `work` folder), never in this repository.
- Default jars hold only hashes (`mcchanges-java.jar`, `mcchanges-bedrock.jar`). With
  `--full` it writes `mcchanges-full-*.jar`, which hold the game files of Mojang. Do not
  publish those.
- Every build is verified by rebuilding each version from the output.

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
