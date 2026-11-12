# Using the fork

This guide shows how to use [my fork of PackSquash](../../README.md). It covers
the quick start, which options are on by default, the mcchanges feature step by
step, and the common problems. Every option is described in full in
[Options files](../wiki/Options-files.md).

## Index

1. [Quick start](#quick-start)
2. [How options work](#how-options-work)
3. [What is on by default and what is not](#what-is-on-by-default-and-what-is-not)
4. [A pack in an archive](#a-pack-in-an-archive)
5. [Bulk: every pack in a folder](#bulk-every-pack-in-a-folder)
6. [mcchanges: leave out what the game already has](#mcchanges-leave-out-what-the-game-already-has)
7. [Language files](#language-files)
8. [Bedrock Edition packs](#bedrock-edition-packs)
9. [OptiFine custom skies](#optifine-custom-skies)
10. [Keep the files of mods](#keep-the-files-of-mods)
11. [Copy files untouched](#copy-files-untouched)
12. [Debug log](#debug-log)
13. [Common problems](#common-problems)
14. [Not built yet](#not-built-yet)

## Quick start

1. Download the program from the
   [Releases](https://github.com/mocny-gaz/PackSquash-uv12/releases) page. The
   zip holds `packsquash.exe`. A release also has the program alone, and a
   `SHA256SUMS.txt` file with the checksums.
2. Make a file named `packsquash.toml` with two lines:

   ```toml
   pack_directory = 'C:\packs\my_pack'
   output_file_path = 'C:\packs\my_pack.zip'
   ```

3. Open cmd and run the program with the options file as its only argument:

   ```
   packsquash.exe packsquash.toml
   ```

PackSquash writes the optimized ZIP file to `output_file_path`. Without any
other option, the fork squashes a pack like the original PackSquash does.

## How options work

- The options file is a [TOML](https://toml.io/en/v1.0.0) file.
- Global options go first, before any table. PackSquash rejects a global option
  that it does not know.
- Options for one kind of file go in a table. The table name is a glob that
  selects the files, like `['assets/minecraft/shaders/core/**']`.
- Write Windows paths in single quotes, like `'C:\amc\mcchanges-java.jar'`. In
  double quotes, the backslash is an escape character.
- Without a path argument, or with `-`, PackSquash reads the options from its
  standard input.
- `packsquash.exe --help` lists the command-line arguments.

## What is on by default and what is not

The options that this fork adds:

| Option | Default | What it does |
| :--- | :--- | :--- |
| `mcchanges_path` | not set, so **off** | Leaves out the pack files that are identical to the game files in this jar, ZIP file, or folder. |
| `mcchanges_version` | not set, so **every version** | The game version to compare with. If it is not set or empty, each file is compared with the same file in every version that changed it. `auto` chooses the one best matching version. An error for any other path. |
| `skip_language_keys_identical_to_mcchanges` | `true` | Removes language lines that have the value of the game. Works only when `mcchanges_path` is set. |
| `skip_unused_optifine_sky_images` | `false` | Leaves out OptiFine sky images that no sky entry uses. Needs `allow_mods = ['OptiFine']`. |
| `merge_identical_optifine_sky_images` | `false` | Stores identical OptiFine sky images once. Needs `allow_mods = ['OptiFine']`. |
| `skip_optifine_sky_entries_with_missing_images` | `false` | Leaves out OptiFine sky entries whose image is not in the pack. Needs `allow_mods = ['OptiFine']`. Not for packs that are still being made. |
| `pack_edition` | `'auto'` | `'auto'` treats a pack with `manifest.json` and no `pack.mcmeta` as a Bedrock Edition pack. Other values are `'java'` and `'bedrock'`. |
| `copy_as_is` (file option) | `false` | When `true`, copies the files that match the glob exactly as they are. |
| `include_unrecognized_asset_files` | `true` | Keeps every file in a folder of `assets` (any namespace, so mods work), copying the ones that PackSquash does not know. Set to `false` to skip them. |
| `copy_failed_files` | `true` | A file that can not be optimized is copied as it is, with a notice, instead of failing the pack. |
| `archive_pack_search_depth` | `4` | How many directory levels are searched for the pack when `pack_directory` is an archive. |
| `archive_max_extracted_mib` | `16384` | The most data, in MiB, that is extracted from an archive. |
| `bulk_input_directory` | not set, so **off** | Squashes every pack found in this folder. |
| `bulk_output_directory` | not set | Where the results go, with the same folders as the input and no prefix. |
| `bulk_output_prefix` | `'squashed_'` (`''` with `bulk_output_directory`) | Start of every result name in the bulk mode. |
| `bulk_output_suffix` | `''` | End of every result name, before `.zip`. |
| `bulk_overwrite` | `false` | Replaces the original archive with the result. Does nothing with `bulk_output_directory`. |
| `bulk_backup` | `true` | With `bulk_overwrite`, keeps the original as `NAME.original`. |
| `bulk_dry_run` | `false` | Only lists what would happen. |
| `bulk_report_file` | not set | Also writes the report to this file. |
| `bulk_archive_extensions` | `'zip,7z,rar,tar,gz,tgz,xz,txz,zst,tzst,bz2,tbz2'` | Files with these extensions are opened as archives. `jar` is not in the list. |
| `bulk_scan_depth` | `32` | How many folder levels are searched, also inside archives. It is also how many archives can be inside each other (at most 32). |
| `log` | `false` | Writes a very detailed debug log file, to send when something goes wrong. |
| `log_file` | `'packsquash-debug-NAME.log'` next to the options file | Where the debug log is written. NAME is the name of the input. |

Things that are always on, with no option:

- Shaders with `#include` (Minecraft 26.3 and later) are accepted.
- Files that your options ask to leave out are not reported as warnings. They
  show up in the trace log level.
- A raw line break or tab inside a string of `pack.mcmeta` or another JSON file
  is accepted, like the game accepts it. JSON files that are really broken
  still fail.

Every other option keeps the default of the original PackSquash.

## A pack in an archive

`pack_directory` can be an archive file instead of a directory:

```toml
pack_directory = 'C:\packs\my_pack.7z'
output_file_path = 'C:\packs\my_pack.zip'
```

- Without other programs: ZIP, 7z, and TAR alone or with gzip, xz, Zstandard or bzip2
  (`.tar`, `.tar.gz`, `.tar.xz`, `.tar.zst`, `.tar.bz2`).
- RAR needs a helper program: `bsdtar`, `tar`, `7z`, `7zz`, `7za` or `unrar`. Windows 10 and
  later have `tar.exe`. If it can not read your file, install 7-Zip and add its folder to the
  `PATH`.
- PackSquash finds the format by looking inside the file. The name does not matter.
- The pack is the directory with `pack.mcmeta` and `assets` or `data`, or with a Bedrock
  `manifest.json`. It can be in a subfolder of the archive, up to `archive_pack_search_depth`
  levels deep.
- One pack per run. If the archive has more, PackSquash lists them and stops.
- The pack is extracted to a temporary folder, which is deleted at the end. You need free disk
  space for the pack. Links and unsafe names in the archive are never extracted.
- The result is always a ZIP file.
- Archives with a password are not supported.

To look deeper, or to allow more data:

```toml
archive_pack_search_depth = 6
archive_max_extracted_mib = 32768
```

## Bulk: every pack in a folder

Set `bulk_input_directory` and PackSquash squashes every pack that it finds there:

```toml
bulk_input_directory = 'C:\packs'
```

- A pack is a folder with `pack.mcmeta` and `assets` or `data`, or an archive that has one
  (see the section above for the formats). Subfolders are searched. A pack folder is not
  searched inside.
- `bulk_input_directory` can also be one archive file, like `C:\packs\big.7z`. Every pack in it
  is squashed, and the results go next to it. The name and extension do not matter.
- Every result is a ZIP file next to its input: `squashed_NAME.zip` by default. For an archive,
  NAME has no extension. `bulk_output_prefix` and `bulk_output_suffix` change the name.
- An archive with one pack gives one file. An archive with several packs, or with archives inside it,
  gives a folder: `squashed_ARCHIVE/`, with the same folders that the archive has. The pack
  `a/b/Pack` of the archive is `squashed_ARCHIVE/a/b/Pack.zip`.
- Archives inside archives are opened too, as many levels as `bulk_scan_depth` (at most 32), unless they are part of a pack. A
  `.zip` with 200 packs that are zips gives 200 results in a folder, not 200 files next to the
  big zip. They are extracted a few at a time and deleted when done.
- To put the results somewhere else, set `bulk_output_directory`. The results keep the folders of the
  input, have no prefix (unless you set one) and nothing is written next to the inputs.
- Each line has the number, the name of the pack and where it is, then the size of its files and of the
  result: `[3/204] Pack (C:\packs\big.zip > inner.zip > Pack) -> ... (12.50 MiB -> 3.10 MiB)`.
- Names that already look like a result are skipped, so a second run does not squash the
  results of the first one.
- All the other options, and the tables of file options, are used for every pack.
  `pack_directory` and `output_file_path` are ignored.
- If one pack fails, PackSquash goes on with the next. At the end it prints a summary, and the
  exit code is 1 if any pack failed.

See first what would happen, and keep a report:

```toml
bulk_input_directory = 'C:\packs'
bulk_dry_run = true
bulk_report_file = 'C:\packs\report.txt'
```

To replace the archives with the squashed packs, and keep each original as `NAME.ext.original`:

```toml
bulk_input_directory = 'C:\packs'
bulk_overwrite = true
```

`bulk_backup = false` deletes the originals instead. This can not be undone, so try the dry run
first. PackSquash never replaces a folder, a file that is not the original pack, or an archive with
several packs.

## mcchanges: leave out what the game already has

Many packs are built from a full copy of the vanilla assets and change only a
few files. The game loads its own copy of every file that the pack lacks. So a
pack file that is identical to the game file only adds size. With mcchanges,
PackSquash leaves those files out.

### 1. Get a jar

Download a ready jar from the Releases of the
[mc-changes](https://github.com/mocny-gaz/mc-changes) repository:

| File | What |
| :--- | :--- |
| `mcchanges-java.jar` | Java Edition, many game versions |
| `mcchanges-bedrock.jar` | Bedrock Edition, the newest releases |

These jars hold only fingerprints (hashes) of the game files, not the files.
The repository also has the script that builds and updates them, with its own
instructions.

A jar made with `--full` holds the game files of Mojang. Keep it on your
computer and do not publish it.

### 2. Set the options

```toml
pack_directory = 'C:\packs\my_pack'
output_file_path = 'C:\packs\my_pack.zip'
mcchanges_path = 'C:\amc\mcchanges-java.jar'
mcchanges_version = '1.21.4'
```

`mcchanges_version` is the game version that your pack is for. To see the
versions that a jar has, open `mcchanges.json` in the jar with any ZIP program
and read the `versions` list.

If you leave `mcchanges_version` out, PackSquash compares each file of your pack with the
file with the same path in every version of the jar that changed it, and leaves the file
out if it is identical to any of them. It says so. A file that is the same as it was in an
older version is left out, even if the game version of your pack has a different file.
Set the version yourself if you want a single version.

If you write `mcchanges_version = 'auto'`, PackSquash compares your pack with every
version of the jar and uses the one version that has the most files identical to
your pack. When two versions are equal, it uses the newer one. It prints what it
chose:

```
mcchanges_version is not set, so PackSquash compared the pack with the 87 game
versions of the mcchanges pack and chose 1.21.4, which has 412 identical pack
files. The next best version is 1.21.3 with 409. To use another version, set
mcchanges_version
```

The `auto` choice is the best match, not always your target version. Set the version
yourself when you know it. If no file of your pack is identical to a file of any version,
PackSquash says so and leaves out no file. Language files do not count for the
choice. The choice takes about as long as squashing without it.

### 3. Pick the right kind of `mcchanges_path`

| `mcchanges_path` points to | `mcchanges_version` |
| :--- | :--- |
| A folder that has `mcchanges-java.jar` (or `mcchanges-bedrock.jar` for Bedrock) with exactly that name, and no `mcchanges.json` of its own | Like the jar inside it. PackSquash says that it uses the jar |
| A jar from `mc-changes`, or any ZIP or folder with a `mcchanges.json` file at its root | Set it, or leave it out for the automatic choice |
| The client `.jar` of one game version, or a folder with its files | **Do not set it.** PackSquash fails if you do |

A folder like `C:\amc\` works if it has `mcchanges-java.jar` in it, with exactly that
name. If the jar has another name, point to the jar file itself.

### How PackSquash compares

- Textures: by size and pixels. The way a file is encoded does not matter.
- JSON files: by contents. Formatting and key order do not matter.
- Other files: byte for byte, or by hash.
- A texture and its `.png.mcmeta` file are left out together or kept together.
- `pack.mcmeta` and `pack.png` are never left out.
- Files in overlay directories are never compared.

### Things to know

- Use the game version of the pack. A wrong version can remove files that
  differ between versions.
- If other packs sit below yours, a file that you leave out lets their file show
  through, not the vanilla one.
- After you change the jar, delete the old result ZIP file before you run
  PackSquash again. It may reuse files from the old one.

## Language files

With `mcchanges_path` set, `.json` and `.lang` language files lose the lines
that have the same value as the game. The game uses its own value for them.
A language file with no line left is left out.

To keep all language lines, turn it off:

```toml
skip_language_keys_identical_to_mcchanges = false
```

Files that you mark with `copy_as_is` are never changed by this option.

## Bedrock Edition packs

PackSquash checks `manifest.json`, optimizes textures, JSON files and Ogg audio,
and copies the other files. With `pack_edition = 'auto'` it finds a Bedrock pack
by itself. Set `pack_edition = 'bedrock'` or `'java'` if it guesses wrong. For
`mcchanges_path`, use `mcchanges-bedrock.jar`. The rules are on the page
[Bedrock Edition packs](../wiki/Bedrock-Edition-packs.md). This part was tested
with automated tests, not in the game.

## OptiFine custom skies

Both options need OptiFine support:

```toml
allow_mods = ['OptiFine']
skip_unused_optifine_sky_images = true
merge_identical_optifine_sky_images = true
```

- `skip_unused_optifine_sky_images` leaves out sky images that no sky entry
  names.
- `merge_identical_optifine_sky_images` keeps the first of several identical
  images in a directory and points the sky entries to it. Unused images are
  found after the merge.

A third option fixes the skies of finished packs:

```toml
allow_mods = ['OptiFine']
skip_optifine_sky_entries_with_missing_images = true
```

If `sky3.properties` names `aurora.png` and the pack does not have that file, old
OptiFine versions ignore the entry, but new versions draw a pink and black texture.
This option leaves such entries out, and prints which ones. It also helps when a pack
is converted to another version.

**Do not use it on a pack that is still being made.** If the image is added later, the
entry has to be written again. PackSquash only checks images in
`assets/minecraft/optifine`, because the game or another pack can provide the others.
OptiFine stops loading entries at the first missing number, so an entry with a later
entry after it is kept, and PackSquash says so. Only custom skies are checked, not
connected textures or custom items.

## Keep the files of mods

The original PackSquash skips the files that it does not know. A pack for a modded game can have
files that only a mod reads, and it would drop them. The fork keeps them. A file that PackSquash
can not optimize (an `.ogg` that is not Vorbis, a `pack.png` that is not a PNG, an invalid `.json`)
is copied as it is, with a notice, and does not fail the pack (`copy_failed_files`).

This is on by default in this fork. To turn it off:

```toml
include_unrecognized_asset_files = false
```

Every file in a folder of `assets`, like `assets/minecraft` or `assets/some_mod`, is then
added to the result. Files that PackSquash does not know are copied as they are. Other files
are still optimized as usual. Files outside of `assets` are still skipped. The result can be
bigger, so use this only when a mod needs it.

## Copy files untouched

Use `copy_as_is` for files that must stay byte for byte as they are. It also
works for files that PackSquash knows how to optimize:

```toml
['assets/minecraft/shaders/core/**']
copy_as_is = true
```

Do not combine it with other options of the same file type for the same files.

## Debug log

If something does not work as it should, turn on the debug log and send the file:

```toml
log = true
```

PackSquash writes `packsquash-debug-NAME.log` next to your options file, and prints where. NAME is the
name of the input: the pack directory, the archive without its extension, or `bulk_input_directory`
(`packsquash-debug-My Packs.log`). Characters that a file name can not have become `_`. The log
has the machine and the command line, your options file, every pack file with what was decided
and why, which hash of the mcchanges pack was compared with which file, and every error with
its causes (why an archive could not be opened, why a Vorbis file could not be read, and so
on). It is replaced in every run, so run again with `log = true` when the problem happens.
`log_file` sets another path. The log has the paths of your files, so read it before you share
it. The console output does not change.

## Common problems

When an error stops PackSquash, it prints a `How to fix` line for the likely causes, a link
to this page, and how to get more details. For the most details, turn on the
[debug log](#debug-log) with `log = true`. For more details on the console, set the log level
and run again. In cmd:

```
set PACKSQUASH_LOG=debug
packsquash.exe packsquash.toml
```

| What you see | Cause and fix |
| :--- | :--- |
| `mcchanges_version is set, but ...` and you set `auto` or a version for a plain game jar | `auto` and versions only work for a jar from `mc-changes`. Remove `mcchanges_version`. |
| `mcchanges_version is set, but the folder ... has no mcchanges.json file` | The folder has no `mcchanges-java.jar` (or `mcchanges-bedrock.jar`) with exactly that name. Rename the jar, or set `mcchanges_path` to the jar file itself, like `C:\amc\mcchanges-java.jar`. The message lists the jar files that the folder has. |
| `mcchanges_version is set, but the file ... has no mcchanges.json file inside` | The file is a game jar, not a jar from `mc-changes`. Remove `mcchanges_version`, or use `mcchanges-java.jar`. |
| `mcchanges_path ... can't be read` | The path does not exist, or PackSquash can not read it. Check the spelling. In cmd, `dir C:\amc\mcchanges-java.jar` must list the file. |
| The version is not found | The jar does not have that version. Read the `versions` list in `mcchanges.json`, or download the newest jar. |
| `control character found while parsing a string` | The pack has a raw line break or tab in a JSON string. Use a build that has this fix (after `0.5.0-fork.1`). |
| Files that you expected to be left out stay in | The version is wrong, or the file really differs from the game file. See which files are skipped: run `set PACKSQUASH_LOG=trace` in cmd, then run the program again. |
| The pack looks wrong in the game | Put the files back with `copy_as_is`, turn off `mcchanges_path`, and open an issue in this repository with the options file. |
| `is a file, but not an archive that PackSquash can read` | `pack_directory` points to a file that is not a ZIP, 7z, TAR or RAR archive. |
| `No pack was found in the archive` | The archive has no `pack.mcmeta` with `assets` or `data`, or the pack is deeper than 4 levels. Raise `archive_pack_search_depth`. |
| `The archive ... has 2 packs` | One run squashes one pack. Extract the one you want, or make an archive with only that pack. |
| `this archive needs a helper program` | RAR files need `bsdtar`, `7z` or `unrar`. Install 7-Zip and add its folder to the `PATH`. |
| `PackSquash can not read this ZIP archive (Multi-disk ZIP files are not supported)` | The ZIP is made in a way that the built-in reader does not support. 7-Zip reads it: install it and add its folder to the `PATH`. |
| `The texture is WxH pixels, and its width or height exceeds the configured maximum size` | The image is bigger than `maximum_width_and_height` (8192 by default). An animated texture with many frames in a tall strip can be that big. With `copy_failed_files`, the file is copied as it is. To optimize it, raise `maximum_width_and_height` for it in a file options table. |
| `The option bulk_... is set, but the bulk mode is off` | Set `bulk_input_directory` too. |
| `the result ... would replace a pack that is an input` | In the bulk mode, an empty prefix and suffix give a result with the name of an input ZIP file. Set a prefix or a suffix, or use `bulk_overwrite`. |
| `another pack already has the result ...` | Two packs in the same folder have the same name, like `X` and `X.zip`. Rename one. |
| `already exists and it is not the pack` | `bulk_overwrite` does not replace a file that is not the original. Move that file. |
| `the backup ... already exists` | A `NAME.ext.original` file is there from an earlier run. Move it, or set `bulk_backup = false`. |
| `the archive is too big` | The pack is bigger than `archive_max_extracted_mib`. Raise it. |

Report problems with the fork in
[this repository](https://github.com/mocny-gaz/PackSquash-uv12/issues), not in
the original one.

## Not built yet

These are planned and are in no release. See [TASKS.md](TASKS.md):

- A simple dark GUI that writes the options file.
