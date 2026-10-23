//! Support for finding OptiFine custom sky images that no custom sky entry uses, which
//! are dead weight in a pack.

use std::path::Path;

use crate::vfs::{IteratorTraversalOptions, VirtualFileSystem, read_file};

#[cfg(test)]
mod tests;

/// The directory where OptiFine looks for custom sky entries (`.properties` files) and the
/// images they use.
const SKY_DIRECTORY: &str = "assets/minecraft/optifine/sky/";

/// Detects the images of OptiFine custom skies that are not used by any custom sky entry.
pub(crate) struct UnusedSkyImageDetector {
	/// The concatenated contents of every custom sky entry, or `None` if there are no
	/// entries or they could not be read. In that case, no image is considered unused.
	sky_entries_text: Option<String>
}

impl UnusedSkyImageDetector {
	/// Reads the custom sky entries of the pack in the given directory.
	pub(crate) async fn scan<V: VirtualFileSystem>(
		vfs: &V,
		pack_directory: &Path,
		iterator_traversal_options: IteratorTraversalOptions
	) -> Self {
		let mut sky_entries_text: Option<String> = None;

		// Errors while iterating the pack files are not handled here: the main pack
		// processing loop will report them
		for pack_file in vfs
			.file_iterator(pack_directory, iterator_traversal_options)
			.flatten()
		{
			let relative_path = pack_file.relative_path.as_str();

			if relative_path.starts_with(SKY_DIRECTORY) && relative_path.ends_with(".properties") {
				let Some(entry_data) = read_file(vfs, &pack_file.file_path).await else {
					// Being unable to know what is used means that nothing can be
					// safely considered unused
					return Self {
						sky_entries_text: None
					};
				};

				let text = sky_entries_text.get_or_insert_with(String::new);
				text.push_str(&String::from_utf8_lossy(&entry_data));
				text.push('\n');
			}
		}

		Self { sky_entries_text }
	}

	/// Checks whether the pack file at the given relative path is a custom sky image that no
	/// custom sky entry uses. An image is considered used if any entry mentions its file name,
	/// which is a conservative approximation of how entries refer to images.
	pub(crate) fn is_unused(&self, relative_path: &str) -> bool {
		let Some(sky_entries_text) = &self.sky_entries_text else {
			return false;
		};

		relative_path
			.strip_prefix(SKY_DIRECTORY)
			.filter(|path_in_sky_directory| path_in_sky_directory.ends_with(".png"))
			.is_some_and(|_| {
				let file_name = relative_path.rsplit('/').next().unwrap_or(relative_path);
				!sky_entries_text.contains(file_name)
			})
	}
}
