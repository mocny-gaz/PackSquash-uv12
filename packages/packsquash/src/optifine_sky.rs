//! Support for debloating OptiFine custom skies: leaving out the images that no custom sky entry
//! uses, and merging identical images, which are dead weight in a pack.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::vfs::{IteratorTraversalOptions, VirtualFileSystem, read_file};

#[cfg(test)]
mod tests;

/// The directory where OptiFine looks for custom sky entries (`.properties` files) and the
/// images they use.
const SKY_DIRECTORY: &str = "assets/minecraft/optifine/sky/";

/// Finds the OptiFine custom sky images that can be left out of a pack, and the changes to the
/// custom sky entries that this requires.
pub(crate) struct SkyImageOptimizer {
	skip_unused_images: bool,
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
fn file_name_occurrences<'a>(
	text: &'a str,
	file_name: &'a str
) -> impl Iterator<Item = usize> + 'a {
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

impl SkyImageOptimizer {
	/// Reads the custom sky entries and images of the pack in the given directory.
	pub(crate) async fn scan<V: VirtualFileSystem>(
		vfs: &V,
		pack_directory: &Path,
		iterator_traversal_options: IteratorTraversalOptions,
		skip_unused_images: bool,
		merge_identical_images: bool
	) -> Self {
		let nothing_to_do = Self {
			skip_unused_images,
			sky_entries_text: None,
			rewritten_entries: HashMap::new(),
			merged_images: HashSet::new()
		};

		let mut entry_paths = BTreeMap::<String, PathBuf>::new();
		let mut image_paths = BTreeMap::<String, PathBuf>::new();

		// Errors while iterating the pack files are not handled here: the main pack
		// processing loop will report them
		for pack_file in vfs
			.file_iterator(pack_directory, iterator_traversal_options)
			.flatten()
		{
			let relative_path = pack_file.relative_path.as_str();

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
			sky_entries_text: Some(sky_entries_text),
			rewritten_entries,
			merged_images
		}
	}

	/// If the pack file at the given relative path is a custom sky image that is not needed,
	/// returns the reason why it can be left out.
	pub(crate) fn skip_reason(&self, relative_path: &str) -> Option<&'static str> {
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

	/// If the pack file at the given relative path is a custom sky entry that mentions merged
	/// images, returns its contents with the references updated.
	pub(crate) fn rewritten_entry(&self, relative_path: &str) -> Option<Arc<[u8]>> {
		self.rewritten_entries.get(relative_path).cloned()
	}
}
