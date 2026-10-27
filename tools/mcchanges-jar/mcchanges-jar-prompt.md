# Task: build or update an mcchanges jar for PackSquash

You build one jar that holds the vanilla files of many Minecraft versions in a compact way. PackSquash reads it with `mcchanges_path` and `mcchanges_version`. You do not write the packing code. The script `build_mcchanges_jar.py` does all of it, including the downloads and the verification. Your job is to run it correctly, check the result, and report.

## Rules

- Never edit the jar by hand and never reimplement the script. If it fails, report the exact error text and the command.
- Never upload the jar anywhere public. Its files belong to Mojang. Keep it on the user's computer.
- Use only the commands below. Do not guess other URLs.
- Needs Python 3 and network access to `piston-meta.mojang.com`, `piston-data.mojang.com`, `resources.download.minecraft.net` (Java) or `github.com` and `codeload.github.com` (Bedrock). If a host is blocked, tell the user which one to allow and stop.

## Java Edition

Build, from release 1.6.1 (the first version with resource packs) to the newest release:

```
python build_mcchanges_jar.py java --output mcchanges-java.jar
```

Only the newest releases (faster):

```
python build_mcchanges_jar.py java --output mcchanges-java.jar --last 10
```

Update after Mojang released a new version. It downloads only the new releases and gives the same file as a full rebuild:

```
python build_mcchanges_jar.py java --output mcchanges-java.jar --update
```

## Bedrock Edition

The files come from the official `Mojang/bedrock-samples` repository, one release per version. Each download is about 150 MB.

```
python build_mcchanges_jar.py bedrock --output mcchanges-bedrock.jar --count 5
python build_mcchanges_jar.py bedrock --output mcchanges-bedrock.jar --update
```

Add `--include-previews` only if the user asks for preview releases.

## Audio

Audio files are big and packs rarely copy them, so by default the jar stores only their hash. Java sounds are hashed with the SHA-1 that the Mojang asset index lists, without downloading them. Do not change this unless the user asks: `--audio none` ignores audio, and `--audio full` stores the files (very big).

## Before you run

1. Add `--dry-run` once. It prints the versions that would be added and downloads nothing. Show it to the user if the list looks wrong.
2. The first build of Java takes a while. The script keeps its downloads in `mcchanges-work/`, so running it again continues where it stopped.

## After you run

The script ends with `wrote <file> (<size> MB), verified <n> version(s)`. That line means every version was rebuilt from the jar and compared with its source. Any line that starts with `error:` means the build failed. Do not deliver a jar after an error.

Report:

1. The command that you ran.
2. The size of the jar, and the first and last version in it. Read them from `mcchanges.json` inside the jar.
3. For an update, the versions that were added.
4. This PackSquash configuration with the newest version filled in:

```toml
mcchanges_path = 'mcchanges-java.jar'
mcchanges_version = '<newest version>'
```

## Prompt to paste later for updates

> Update my PackSquash mcchanges jar. `build_mcchanges_jar.py` and the current jar are attached. Follow `mcchanges-jar-prompt.md`, use the Java (or Bedrock) update command, and report what was added.
