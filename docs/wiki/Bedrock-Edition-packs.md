# Bedrock Edition packs

PackSquash can optimize Minecraft Bedrock Edition resource packs. This support was
requested in [issue #266](https://github.com/ComunidadAylas/PackSquash/issues/266).
It reuses the optimizers PackSquash has for Java Edition packs, but only the ones
that are safe for the Bedrock Edition formats. Everything else is copied untouched,
so the pack keeps working.

> [!IMPORTANT]
> The rules below were written from the documentation of the Bedrock Edition pack
> formats and tested with automated tests. They were not tested by loading a
> processed pack in the game. Please check your pack in the game before you
> distribute it.

## How to use it

Point `pack_directory` to the folder that holds `manifest.json`, and choose a
`.mcpack` file as the output. PackSquash detects the edition by itself, but you can
set [`pack_edition`](Options-files.md#pack_edition) to `'bedrock'` to be explicit.

```toml
pack_directory = 'my_bedrock_pack'
output_file_path = 'my_pack.mcpack'
pack_edition = 'bedrock'
```

A `.mcpack` file is a ZIP file with another extension. The pack files are at the
root of the ZIP file, not in a folder, which is also what PackSquash generates.

## What PackSquash does with each file

| File | What happens |
| :--- | :--- |
| `manifest.json`, `contents.json` | Copied as-is. |
| `pack_icon.png`, `bug_pack_icon.png` | Optimized as textures. [`skip_pack_icon`](Options-files.md#skip_pack_icon) leaves them out. |
| Any `.png` file | Losslessly optimized, with the PNG options of your options file. |
| Any `.json` file, and `blocks.json`, `sounds.json`, `biomes_client.json` | Minified. Comments are removed. The order of the keys is kept. |
| Any `.ogg` file | Optimized as Ogg Vorbis audio, with the audio options of your options file. |
| Any other file in a folder (`.lang`, `.tga`, `.wav`, `.fsb`, `.material`...) | Copied as-is. |
| Any other file at the root of the pack (`README.txt`, `packsquash.toml`...) | Skipped, and reported as skipped. |

To include a file that PackSquash skips, or to copy a file as-is that PackSquash
would otherwise change, add it to your options file with `force_include`:

```toml
['README.txt']
force_include = true

['entity/lenient.json']
force_include = true
```

## Checks

If [`validate_pack_metadata_file`](Options-files.md#validate_pack_metadata_file) is
enabled, which is the default, PackSquash reads `manifest.json` and stops with an
error if it is not valid:

- `format_version` is an integer.
- `header` has a `name`, a `uuid` and a `version` (three numbers, or a semantic
  version string if `format_version` is 2).
- Every module has a `type`, a `uuid` and a `version`.
- No `uuid` is repeated, including the one in `header`. The game refuses to load
  packs that repeat them.

PackSquash only warns if `min_engine_version` is missing, if there is no
`resources` module, or if `format_version` is not 1 or 2.

## Differences with Java Edition packs

- **ZIP files:** `zip_spec_conformance_level` must be `'pedantic'`, which is the
  default. Other levels use ZIP file structures that were not checked with Bedrock
  Edition, so PackSquash refuses them.
- **Textures:** PackSquash never changes the color of transparent pixels. Some
  Bedrock textures use the alpha channel for other data, like the subsurface
  scattering of ray tracing textures. You can opt in with
  `skip_alpha_optimizations = false` in the PNG options of a file.
- **JSON:** comments are accepted in every `.json` file. Trailing commas are not
  valid JSON, so PackSquash reports an error. If the game accepts such a file,
  copy it as-is with `force_include`. Numbers are written in their shortest form
  (`2.50` becomes `2.5`, and `3e2` becomes `300.0`), which has the same value.
  Integers that do not fit in 64 bits lose precision.
- **Language files:** `.lang` files are copied as-is. Their comment syntax differs
  from the Java Edition files, so the Java Edition optimizer is not used.
- **Audio:** only `.ogg` files are optimized. Other audio formats are copied as-is
  and never converted. Ogg files with more than two channels are rejected.
- **Pack layers and mods:** pack overlays, quirks, OptiFine and Minecraft Transit
  Railway options do not apply.

## Using a baseline pack

[`baseline_pack_path`](Options-files.md#baseline_pack_path) also works with Bedrock
Edition packs. For example, you can point it to the official sample resource pack
to leave out the files you did not change. The `manifest.json`, `pack_icon.png`
and `bug_pack_icon.png` files are never left out. Language key removal only works
for Java Edition language files.
