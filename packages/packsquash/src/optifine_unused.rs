//! Finds the OptiFine entries that are not connected to anything: entries that use files that are missing
//! from the pack, and connected textures tiles that no entry uses.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::debug_log::detail;
use crate::vfs::{IteratorTraversalOptions, VirtualFileSystem, read_file};

#[cfg(test)]
mod tests;

const ROOTS: [&str; 2] = ["assets/minecraft/optifine/", "assets/minecraft/mcpatcher/"];

const TILE_REASON: &str = "Unused by OptiFine connected textures entries";

#[derive(Clone, Copy)]
enum Feature {
	ConnectedTextures,
	CustomItems,
	Animations,
	Gui,
	Colormaps
}

impl Feature {
	fn of(entry_path: &str) -> Option<(Self, &'static str)> {
		let root = ROOTS
			.into_iter()
			.find(|root| entry_path.starts_with(root))?;
		let feature = match entry_path[root.len()..].split_once('/')?.0 {
			"ctm" => Self::ConnectedTextures,
			"cit" => Self::CustomItems,
			"anim" => Self::Animations,
			"gui" => Self::Gui,
			"colormap" => Self::Colormaps,
			_ => return None
		};

		Some((feature, root))
	}

	fn skip_reason(self) -> &'static str {
		match self {
			Self::ConnectedTextures => "OptiFine connected textures entry whose tiles are missing",
			Self::CustomItems => "OptiFine custom items entry whose files are missing",
			Self::Animations => "OptiFine custom animation whose image is missing",
			Self::Gui => "OptiFine custom GUI entry whose textures are missing",
			Self::Colormaps => "OptiFine custom colors entry whose image is missing"
		}
	}
}

pub(crate) struct OptifineUnused {
	skipped: HashMap<String, &'static str>,
	notices: Vec<String>
}

fn directory_of(relative_path: &str) -> &str {
	relative_path
		.rsplit_once('/')
		.map_or("", |(directory, _)| &relative_path[..=directory.len()])
}

fn file_name_of(relative_path: &str) -> &str {
	relative_path
		.rsplit_once('/')
		.map_or(relative_path, |(_, file_name)| file_name)
}

fn is_numbered_tile(relative_path: &str) -> bool {
	ROOTS
		.iter()
		.any(|root| relative_path.starts_with(&format!("{root}ctm/")))
		&& file_name_of(relative_path)
			.strip_suffix(".png")
			.is_some_and(|number| {
				!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
			})
}

fn properties(text: &str) -> Option<HashMap<&str, &str>> {
	if text.lines().any(|line| line.ends_with('\\')) {
		return None;
	}

	let mut properties = HashMap::new();

	for line in text.lines() {
		let line = line.trim_start();

		if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
			continue;
		}

		let key_end = line
			.find(|c: char| c == '=' || c == ':' || c.is_whitespace())
			.unwrap_or(line.len());
		let value = line[key_end..].trim_start();
		let value = value
			.strip_prefix(['=', ':'])
			.map_or(value, str::trim_start)
			.trim_end();

		properties.insert(&line[..key_end], value);
	}

	Some(properties)
}

/// The paths that a value can mean, if the first one can only be provided by this pack: the game has no OptiFine files,
/// but it has the other textures and models, and other packs can provide the rest.
fn resolve(entry_path: &str, root: &str, value: &str, extension: &str) -> Option<Vec<String>> {
	if value.is_empty() || value.contains("..") || value.contains("//") || value.contains('\\') {
		return None;
	}

	let value = value
		.strip_prefix('/')
		.filter(|rest| rest.starts_with("~/"))
		.unwrap_or(value);
	let directory = directory_of(entry_path);

	let candidates = if let Some(rest) = value.strip_prefix("./") {
		vec![format!("{directory}{rest}")]
	} else if let Some(rest) = value.strip_prefix("~/") {
		vec![format!("{root}{rest}")]
	} else if let Some(rest) = value.strip_prefix("assets/minecraft/") {
		vec![format!("assets/minecraft/{rest}")]
	} else if value.starts_with('/') {
		return None;
	} else if let Some((namespace, rest)) = value.split_once(':') {
		(namespace == "minecraft").then(|| vec![format!("assets/minecraft/{rest}")])?
	} else if !value.contains('/') {
		vec![
			format!("{directory}{value}"),
			format!("assets/minecraft/{value}"),
		]
	} else {
		vec![format!("assets/minecraft/{value}")]
	};

	let candidates: Vec<String> = candidates
		.into_iter()
		.map(|path| {
			if path.ends_with(extension) {
				path
			} else {
				format!("{path}{extension}")
			}
		})
		.collect();

	ROOTS
		.iter()
		.any(|root| candidates[0].starts_with(root))
		.then_some(candidates)
}

const MAX_TILES: u32 = 2048;

fn tile_references(entry_path: &str, root: &str, tiles: &str) -> Vec<Option<Vec<String>>> {
	let mut references = Vec::new();

	for token in tiles.split_whitespace() {
		if token == "<skip>" {
			continue;
		}

		let range = match token.split_once('-') {
			Some((first, last)) => first.parse::<u32>().ok().zip(last.parse::<u32>().ok()),
			None => token.parse::<u32>().ok().map(|number| (number, number))
		};

		match range {
			Some((first, last)) if first <= last && last - first <= MAX_TILES => {
				for number in first..=last {
					references.push(resolve(entry_path, root, &number.to_string(), ".png"));
				}
			}
			Some(_) => references.push(None),
			None if token == "<default>" => references.push(None),
			None => references.push(resolve(entry_path, root, token, ".png"))
		}
	}

	references
}

fn references(
	feature: Feature,
	entry_path: &str,
	root: &str,
	properties: &HashMap<&str, &str>
) -> Vec<Option<Vec<String>>> {
	let own_file = || {
		file_name_of(entry_path)
			.strip_suffix(".properties")
			.and_then(|name| resolve(entry_path, root, &format!("./{name}"), ".png"))
	};
	let keyed = |key: &str, extension: &str| {
		properties
			.iter()
			.filter(|(name, _)| **name == key || name.starts_with(&format!("{key}.")))
			.map(|(_, value)| resolve(entry_path, root, value, extension))
			.collect::<Vec<_>>()
	};

	match feature {
		Feature::ConnectedTextures => properties
			.get("tiles")
			.map(|tiles| tile_references(entry_path, root, tiles))
			.unwrap_or_default(),
		Feature::CustomItems => {
			let mut references = keyed("texture", ".png");

			references.extend(keyed("model", ".json"));

			if references.is_empty() {
				references.push(own_file());
			}

			references
		}
		Feature::Animations => properties
			.get("from")
			.map(|from| vec![resolve(entry_path, root, from, ".png")])
			.unwrap_or_default(),
		Feature::Gui => keyed("texture", ".png"),
		Feature::Colormaps if properties.get("format") == Some(&"fixed") => Vec::new(),
		Feature::Colormaps => vec![match properties.get("source") {
			Some(source) => resolve(entry_path, root, source, ".png"),
			None => own_file()
		}]
	}
}

impl OptifineUnused {
	pub(crate) async fn scan<V: VirtualFileSystem>(
		vfs: &V,
		pack_directory: &Path,
		iterator_traversal_options: IteratorTraversalOptions
	) -> Self {
		let mut all_paths = HashSet::<String>::new();
		let mut entry_paths = BTreeMap::<String, (PathBuf, Feature, &'static str)>::new();
		let mut tile_paths = Vec::<String>::new();

		for pack_file in vfs
			.file_iterator(pack_directory, iterator_traversal_options)
			.flatten()
		{
			let relative_path = pack_file.relative_path.as_str();

			all_paths.insert(relative_path.to_lowercase());

			if relative_path.ends_with(".properties")
				&& let Some((feature, root)) = Feature::of(relative_path)
			{
				entry_paths.insert(
					relative_path.to_owned(),
					(pack_file.file_path, feature, root)
				);
			} else if is_numbered_tile(relative_path) {
				tile_paths.push(relative_path.to_owned());
			}
		}

		let mut skipped = HashMap::new();
		let mut referenced = HashSet::<String>::new();
		let mut entry_directories = HashSet::<String>::new();
		let mut unclear_directories = HashSet::<String>::new();
		let mut dead_entries = 0_usize;
		let mut unused_tiles = 0_usize;

		for (entry_path, (file_path, feature, root)) in entry_paths {
			let directory = directory_of(&entry_path).to_owned();

			entry_directories.insert(directory.clone());

			let data = read_file(vfs, &file_path).await;
			let text = data
				.as_deref()
				.map(|data| String::from_utf8_lossy(data).into_owned());
			let properties = text
				.as_deref()
				.and_then(|text| properties(text.trim_start_matches('\u{feff}')));
			let Some(properties) = properties else {
				detail!("OptiFine entry {entry_path}: it can't be read for sure, so it is kept");
				unclear_directories.insert(directory);

				continue;
			};

			let references = references(feature, &entry_path, root, &properties);
			let mut dead = !references.is_empty();

			for reference in &references {
				match reference {
					None => {
						dead = false;
						unclear_directories.insert(directory.clone());
					}
					Some(candidates) => {
						for candidate in candidates {
							referenced.insert(candidate.to_lowercase());
						}

						dead &= !candidates
							.iter()
							.any(|candidate| all_paths.contains(&candidate.to_lowercase()));
					}
				}
			}

			detail!(
				"OptiFine entry {entry_path}: {} references, {}",
				references.len(),
				if dead {
					"all of them are files that only this pack can provide, and are not in the pack"
				} else {
					"it is kept"
				}
			);

			if dead {
				skipped.insert(entry_path, feature.skip_reason());
				dead_entries += 1;
			}
		}

		for tile in tile_paths {
			let directory = directory_of(&tile);

			if entry_directories.contains(directory)
				&& !unclear_directories.contains(directory)
				&& !referenced.contains(&tile.to_lowercase())
			{
				detail!("OptiFine connected textures tile {tile}: no entry uses it");
				skipped.insert(format!("{tile}.mcmeta"), TILE_REASON);
				skipped.insert(tile, TILE_REASON);
				unused_tiles += 1;
			}
		}

		let mut parts = Vec::new();

		if dead_entries > 0 {
			parts.push(format!(
				"{dead_entries} OptiFine entries whose files are missing from the pack"
			));
		}

		if unused_tiles > 0 {
			parts.push(format!(
				"{unused_tiles} connected textures tiles that no entry uses"
			));
		}

		let notices = if parts.is_empty() {
			Vec::new()
		} else {
			vec![format!(
				"Left out {}. Do not use optifine_unused on a pack that is still being made: a file that is added later is not used by the entries that were left out.",
				parts.join(" and ")
			)]
		};

		Self { skipped, notices }
	}

	pub(crate) fn skip_reason(&self, relative_path: &str) -> Option<&'static str> {
		self.skipped.get(relative_path).copied()
	}

	pub(crate) fn notices(&self) -> &[String] {
		&self.notices
	}
}
