# Using the fork

This guide shows how to use [my fork of PackSquash](../../README.md). It covers
the quick start, which options are on by default, the mcchanges feature step by
step, and the common problems. Every option is described in full in
[Options files](../wiki/Options-files.md).

## Index

1. [Quick start](#quick-start)
2. [How options work](#how-options-work)
3. [What is on by default and what is not](#what-is-on-by-default-and-what-is-not)
4. [mcchanges: leave out what the game already has](#mcchanges-leave-out-what-the-game-already-has)
5. [Language files](#language-files)
6. [Bedrock Edition packs](#bedrock-edition-packs)
7. [OptiFine custom skies](#optifine-custom-skies)
8. [Copy files untouched](#copy-files-untouched)
9. [Common problems](#common-problems)
10. [Not built yet](#not-built-yet)

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
| `mcchanges_version` | not set | The game version to compare with. Required for a jar that holds many versions. An error for any other path. |
| `skip_language_keys_identical_to_mcchanges` | `true` | Removes language lines that have the value of the game. Works only when `mcchanges_path` is set. |
| `skip_unused_optifine_sky_images` | `false` | Leaves out OptiFine sky images that no sky entry uses. Needs `allow_mods = ['OptiFine']`. |
| `merge_identical_optifine_sky_images` | `false` | Stores identical OptiFine sky images once. Needs `allow_mods = ['OptiFine']`. |
| `pack_edition` | `'auto'` | `'auto'` treats a pack with `manifest.json` and no `pack.mcmeta` as a Bedrock Edition pack. Other values are `'java'` and `'bedrock'`. |
| `copy_as_is` (file option) | `false` | When `true`, copies the files that match the glob exactly as they are. |

Things that are always on, with no option:

- Shaders with `#include` (Minecraft 26.3 and later) are accepted.
- Files that your options ask to leave out are not reported as warnings. They
  show up in the trace log level.
- A raw line break or tab inside a string of `pack.mcmeta` or another JSON file
  is accepted, like the game accepts it. JSON files that are really broken
  still fail.

Every other option keeps the default of the original PackSquash.

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

`mcchanges_version` must be the game version that your pack is for. To see the
versions that a jar has, open `mcchanges.json` in the jar with any ZIP program
and read the `versions` list.

### 3. Pick the right kind of `mcchanges_path`

| `mcchanges_path` points to | `mcchanges_version` |
| :--- | :--- |
| A jar from `mc-changes`, or any ZIP or folder with a `mcchanges.json` file at its root | **Required** |
| The client `.jar` of one game version, or a folder with its files | **Do not set it.** PackSquash fails if you do |

A folder like `C:\amc\` is not a jar from `mc-changes`. Point to the jar file
itself.

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

## Copy files untouched

Use `copy_as_is` for files that must stay byte for byte as they are. It also
works for files that PackSquash knows how to optimize:

```toml
['assets/minecraft/shaders/core/**']
copy_as_is = true
```

Do not combine it with other options of the same file type for the same files.

## Common problems

| What you see | Cause and fix |
| :--- | :--- |
| `mcchanges_version` error | The path is a folder or a single-version jar. Remove `mcchanges_version`, or point to a jar from `mc-changes`. |
| The version is not found | The jar does not have that version. Read the `versions` list in `mcchanges.json`, or download the newest jar. |
| `control character found while parsing a string` | The pack has a raw line break or tab in a JSON string. Use a build that has this fix (after `0.5.0-fork.1`). |
| Files that you expected to be left out stay in | The version is wrong, or the file really differs from the game file. See which files are skipped: run `set PACKSQUASH_LOG=trace` in cmd, then run the program again. |
| The pack looks wrong in the game | Put the files back with `copy_as_is`, turn off `mcchanges_path`, and open an issue in this repository with the options file. |
| `rar` or `7z` input does not work | Archive input is not built yet. See the next section. |

Report problems with the fork in
[this repository](https://github.com/mocny-gaz/PackSquash-uv12/issues), not in
the original one.

## Not built yet

These are planned and are in no release. See [TASKS.md](TASKS.md):

- A pack as an archive of any kind (`.7z`, `.tar`, `.rar` and others) as input.
- Bulk squash of every pack in a directory.
- A blank `mcchanges_version` that tries every version and uses the best match.
- A simple dark GUI that writes the options file.
