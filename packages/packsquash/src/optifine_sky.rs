//! Support for debloating and fixing OptiFine custom skies: leaving out the images that no custom sky
//! entry uses, merging identical images, which are dead weight in a pack, and leaving out the
//! custom sky entries whose image is not in the pack.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::debug_log::detail;
use crate::vfs::{IteratorTraversalOptions, VirtualFileSystem, read_file};

#[cfg(test)]
mod tests;

/// The directory where OptiFine looks for custom sky entries (`.properties` files) and the
/// images they use.
const SKY_DIRECTORY: &str = "assets/minecraft/optifine/sky/";

/// The directory of the files that only OptiFine knows. The game does not have any file in it.
const OPTIFINE_DIRECTORY: &str = "assets/minecraft/optifine/";

/// Finds the OptiFine custom sky images that can be left out of a pack, and the changes to the
/// custom sky entries that this requires.
pub(crate) struct SkyImageOptimizer {
	skip_unused_images: bool,
	/// The custom sky entries that are left out because the image that they use is not in the pack.
	skipped_entries: HashSet<String>,
	/// Messages that tell the user what the optimizer found about the custom sky entries.
	notices: Vec<String>,
	/// The concatenated contents of every custom sky entry, after merging images, or `None` if
	/// there are no entries or they could not be read. In that case, no image is considered
	/// unused.
	sky_entries_text: Option<String>,
	/// The custom sky entries that mention merged images, with the references updated.
	rewritten_entries: HashMap<String, Arc<[u8]>>,
	/// The images that are identical to another image of the same directory, and are left out
	/// because their references point to that other image.
	merged_images: HashSet<String>
}

/// Returns the directory part of a relative path, including the trailing slash.
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

/// Finds the byte offsets where `file_name` appears in `text` as a whole path component or
/// value, so `b.png` is found in `source=b.png` and `source=./b.png`, but not in `source=ab.png`.
fn file_name_occurrences<'a>(text: &'a str, file_name: &'a str) -> impl Iterator<Item = usize> + 'a {
	text.match_indices(file_name)
		.map(|(offset, _)| offset)
		.filter(|&offset| {
			let before = offset.checked_sub(1).map(|index| text.as_bytes()[index]);
			let after = text.as_bytes().get(offset + file_name.len()).copied();

			before.is_none_or(|byte| matches!(byte, b'/' | b'=' | b':') || byte.is_ascii_whitespace())
				&& after.is_none_or(|byte| byte.is_ascii_whitespace())
		})
}

/// Replaces the occurrences of a file name that [`file_name_occurrences`] finds.
fn replace_file_name(text: &str, from: &str, to: &str) -> String {
	let mut replaced = String::with_capacity(text.len());
	let mut copied_until = 0;

	for offset in file_name_occurrences(text, from) {
		replaced.push_str(&text[copied_until..offset]);
		replaced.push_str(to);
		copied_until = offset + from.len();
	}
	replaced.push_str(&text[copied_until..]);

	replaced
}

/// What a custom sky entry says about the image that it uses.
#[derive(Debug, PartialEq, Eq)]
enum SourceProperty<'a> {
	/// The entry does not set the image, so OptiFine uses its default.
	Unset,
	/// The image that the entry sets.
	Set(&'a str),
	/// The entry is written in a way that is not understood well enough to tell.
	Unclear
}

/// Reads the `source` property of the text of a custom sky entry, which is a Java properties text.
/// If the property is set more than once, the last one is the one that counts.
fn source_property(text: &str) -> SourceProperty<'_> {
	// A line that continues in the next one could hide or change a property
	if text.lines().any(|line| line.ends_with('\\')) {
		return SourceProperty::Unclear;
	}

	let mut source = SourceProperty::Unset;

	for line in text.lines() {
		let line = line.trim_start();

		if line.is_empty() || line.starts_with('#') || line.starts_with('!') {
			continue;
		}

		let key_end = line
			.find(|c: char| c == '=' || c == ':' || c.is_whitespace())
			.unwrap_or(line.len());

		if &line[..key_end] != "source" {
			continue;
		}

		let value = line[key_end..].trim_start();
		let value = value
			.strip_prefix(['=', ':'])
			.map_or(value, str::trim_start)
			.trim_end();

		source = SourceProperty::Set(value);
	}

	source
}

/// Finds the path in the pack of the image that a custom sky entry uses, following the rules of
/// OptiFine for paths: `path/file` is relative to `assets/minecraft`, `~/path/file` to
/// `assets/minecraft/optifine`, `./path/file` to the directory of the entry, and
/// `namespace:path/file` to `assets/namespace`. If the entry does not set an image, `sky<n>.png`
/// next to it is used. Returns `None` if the path can't be told for sure.
fn source_image_path(entry_path: &str, source: &SourceProperty<'_>) -> Option<String> {
	let directory = directory_of(entry_path);

	let source = match source {
		SourceProperty::Unclear => return None,
		SourceProperty::Unset => {
			let number = file_name_of(entry_path)
				.strip_prefix("sky")?
				.strip_suffix(".properties")?;

			return (!number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit()))
				.then(|| format!("{directory}sky{number}.png"));
		}
		SourceProperty::Set(source) => *source
	};

	if source.is_empty() {
		return None;
	}

	if source.contains("..") || source.contains("//") || source.contains('\\') {
		return None;
	}

	let source = match source.strip_prefix('/') {
		Some(rest) if rest.starts_with("~/") => rest,
		_ => source
	};

	if let Some(rest) = source.strip_prefix("assets/minecraft/") {
		Some(format!("assets/minecraft/{rest}"))
	} else if let Some(rest) = source.strip_prefix("./") {
		Some(format!("{directory}{rest}"))
	} else if let Some(rest) = source.strip_prefix("~/") {
		Some(format!("assets/minecraft/optifine/{rest}"))
	} else if let Some((namespace, rest)) = source.split_once(':') {
		// Another namespace can be provided by another pack, so it can't be told
		(namespace == "minecraft").then(|| format!("assets/minecraft/{rest}"))
	} else if source.starts_with('/') {
		None
	} else {
		Some(format!("assets/minecraft/{source}"))
	}
}

/// Returns the number of a custom sky entry that is part of the numbered sequence of layers that
/// OptiFine loads, like 3 for `sky3.properties`.
fn layer_number(entry_path: &str) -> Option<u32> {
	file_name_of(entry_path)
		.strip_prefix("sky")?
		.strip_suffix(".properties")?
		.parse()
		.ok()
}

impl SkyImageOptimizer {
	/// Reads the custom sky entries and images of the pack in the given directory.
	pub(crate) async fn scan<V: VirtualFileSystem>(
		vfs: &V,
		pack_directory: &Path,
		iterator_traversal_options: IteratorTraversalOptions,
		skip_unused_images: bool,
		merge_identical_images: bool,
		skip_missing_image_entries: bool
	) -> Self {
		let nothing_to_do = Self {
			skip_unused_images,
			skipped_entries: HashSet::new(),
			notices: Vec::new(),
			sky_entries_text: None,
			rewritten_entries: HashMap::new(),
			merged_images: HashSet::new()
		};

		let mut entry_paths = BTreeMap::<String, PathBuf>::new();
		let mut image_paths = BTreeMap::<String, PathBuf>::new();
		let mut all_paths = HashSet::<String>::new();

		// Errors while iterating the pack files are not handled here: the main pack
		// processing loop will report them
		for pack_file in vfs
			.file_iterator(pack_directory, iterator_traversal_options)
			.flatten()
		{
			let relative_path = pack_file.relative_path.as_str();

			if skip_missing_image_entries {
				all_paths.insert(relative_path.to_owned());
			}

			if relative_path.starts_with(SKY_DIRECTORY) {
				if relative_path.ends_with(".properties") {
					entry_paths.insert(relative_path.to_owned(), pack_file.file_path);
				} else if relative_path.ends_with(".png") && merge_identical_images {
					image_paths.insert(relative_path.to_owned(), pack_file.file_path);
				}
			}
		}

		if entry_paths.is_empty() {
			return nothing_to_do;
		}

		let mut entries = BTreeMap::<String, String>::new();
		for (relative_path, file_path) in entry_paths {
			let Some(data) = read_file(vfs, &file_path).await else {
				// Being unable to know what is used means that nothing can be safely
				// considered unused or merged
				return nothing_to_do;
			};

			entries.insert(relative_path, String::from_utf8_lossy(&data).into_owned());
		}

		// Leave out the entries whose image is not in the pack, if the user wants it
		let mut skipped_entries = HashSet::new();
		let mut notices = Vec::new();

		if skip_missing_image_entries {
			// An entry is only considered when the image is a file that only this pack can provide.
			// The game has no OptiFine files, but it does have the other textures
			let missing_image_of = |entry_path: &str, text: &str| {
				let source = source_property(text);
				let image_path = source_image_path(entry_path, &source);
				let missing = image_path.clone().filter(|image_path| {
					image_path.starts_with(OPTIFINE_DIRECTORY) && !all_paths.contains(image_path)
				});

				detail!(
					"OptiFine sky entry {entry_path}: source property {source:?}, image resolved to {}, {}",
					image_path
						.as_deref()
						.unwrap_or("nothing, because the path can't be told for sure"),
					if missing.is_some() {
						"the image is not in the pack"
					} else {
						"the image is in the pack, or it could be provided by the game or another pack"
					}
				);

				missing
			};

			// OptiFine loads the numbered entries of a directory in order, and stops at the first number
			// that does not exist. Leaving out an entry that has a later one would hide the later ones, so
			// an entry is only left out if every entry that comes after it is left out too
			let mut entries_by_directory = BTreeMap::<&str, Vec<(Option<u32>, &str)>>::new();
			for entry_path in entries.keys() {
				entries_by_directory
					.entry(directory_of(entry_path))
					.or_default()
					.push((layer_number(entry_path), entry_path));
			}

			for (_, directory_entries) in entries_by_directory {
				let mut numbered = directory_entries
					.iter()
					.filter_map(|(number, path)| number.map(|number| (number, *path)))
					.collect::<Vec<_>>();
				numbered.sort_unstable();

				let mut later_entry_is_kept: Option<&str> = None;

				for (_, entry_path) in numbered.into_iter().rev() {
					match (
						missing_image_of(entry_path, &entries[entry_path]),
						later_entry_is_kept
					) {
						(Some(image_path), None) => {
							notices.push(format!(
								"OptiFine custom sky entry {entry_path} was left out, because its image {image_path} is not in the pack"
							));
							skipped_entries.insert(entry_path.to_owned());
						}
						(Some(image_path), Some(later_entry)) => {
							notices.push(format!(
								"OptiFine custom sky entry {entry_path} was kept, although its image {image_path} is not in the pack, because {later_entry} comes after it and OptiFine stops loading entries at the first missing number"
							));
							later_entry_is_kept = Some(entry_path);
						}
						(None, _) => later_entry_is_kept = Some(entry_path)
					}
				}

				// The entries that are not part of a numbered sequence do not depend on the others
				for (_, entry_path) in directory_entries
					.iter()
					.filter(|(number, _)| number.is_none())
				{
					if let Some(image_path) = missing_image_of(entry_path, &entries[*entry_path]) {
						notices.push(format!(
							"OptiFine custom sky entry {entry_path} was left out, because its image {image_path} is not in the pack"
						));
						skipped_entries.insert((*entry_path).to_owned());
					}
				}
			}

			notices.sort();

			if !skipped_entries.is_empty() {
				notices.push(
					"Custom sky entries were left out because their image is missing. If the image is not added \
					to the pack yet, the entry has to be added again. Do not use \
					skip_optifine_sky_entries_with_missing_images on a pack that is still being made"
						.to_owned()
				);
			}

			entries.retain(|entry_path, _| !skipped_entries.contains(entry_path));
		}

		// Group the identical images of each directory. The first image in path order is
		// kept, and the others are candidates to be merged into it
		let mut kept_images = HashMap::<(&str, [u8; 32], usize), &str>::new();
		let mut merge_candidates = Vec::<(&str, &str)>::new();

		let mut image_contents = BTreeMap::<&str, Vec<u8>>::new();
		for (relative_path, file_path) in &image_paths {
			let Some(data) = read_file(vfs, file_path).await else {
				return nothing_to_do;
			};
			image_contents.insert(relative_path, data);
		}

		for (relative_path, data) in &image_contents {
			let key = (
				directory_of(relative_path),
				Sha256::digest(data).into(),
				data.len()
			);

			match kept_images.get(&key) {
				Some(kept_image) => merge_candidates.push((relative_path, kept_image)),
				None => {
					kept_images.insert(key, relative_path);
				}
			}
		}

		let mut merged_images = HashSet::new();
		let mut changed_entries = HashSet::new();

		for (merged_image, kept_image) in merge_candidates {
			let (directory, merged_name, kept_name) = (
				directory_of(merged_image),
				file_name_of(merged_image),
				file_name_of(kept_image)
			);

			// An entry in another directory may refer to the image in a way that can't be
			// told apart from a reference to a file with the same name in its own directory.
			// Merging could then change what that entry uses, so keep the image
			if entries.iter().any(|(entry_path, text)| {
				directory_of(entry_path) != directory
					&& file_name_occurrences(text, merged_name).next().is_some()
			}) {
				continue;
			}

			for (entry_path, text) in entries
				.iter_mut()
				.filter(|(entry_path, _)| directory_of(entry_path) == directory)
			{
				if file_name_occurrences(text, merged_name).next().is_some() {
					*text = replace_file_name(text, merged_name, kept_name);
					changed_entries.insert(entry_path.clone());
				}
			}

			merged_images.insert(merged_image.to_owned());
		}

		let sky_entries_text = entries.values().fold(String::new(), |mut text, entry| {
			text.push_str(entry);
			text.push('\n');
			text
		});

		let rewritten_entries = changed_entries
			.into_iter()
			.map(|entry_path| {
				let data = Arc::from(entries[&entry_path].as_bytes());
				(entry_path, data)
			})
			.collect();

		Self {
			skip_unused_images,
			skipped_entries,
			notices,
			sky_entries_text: Some(sky_entries_text),
			rewritten_entries,
			merged_images
		}
	}

	/// If the pack file at the given relative path is a custom sky image that is not needed,
	/// returns the reason why it can be left out.
	pub(crate) fn skip_reason(&self, relative_path: &str) -> Option<&'static str> {
		if self.skipped_entries.contains(relative_path) {
			return Some("OptiFine custom sky entry whose image is missing");
		}

		if self.merged_images.contains(relative_path) {
			return Some("Merged with an identical OptiFine custom sky image");
		}

		let sky_entries_text = self.sky_entries_text.as_ref()?;

		(self.skip_unused_images
			&& relative_path
				.strip_prefix(SKY_DIRECTORY)
				.is_some_and(|path_in_sky_directory| path_in_sky_directory.ends_with(".png"))
			&& !sky_entries_text.contains(file_name_of(relative_path)))
		.then_some("Unused by OptiFine custom sky entries")
	}

	/// The messages that tell the user which custom sky entries were left out or kept because their
	/// image is missing, and a warning about it.
	pub(crate) fn notices(&self) -> &[String] {
		&self.notices
	}

	/// If the pack file at the given relative path is a custom sky entry that mentions merged
	/// images, returns its contents with the references updated.
	pub(crate) fn rewritten_entry(&self, relative_path: &str) -> Option<Arc<[u8]>> {
		self.rewritten_entries.get(relative_path).cloned()
	}
}
