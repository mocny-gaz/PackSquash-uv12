//! Support for mcchanges packs: packs, usually the vanilla game assets, that the pack being
//! processed is layered on top of. Pack files that do not change anything relative to the
//! mcchanges are redundant, so they can be left out of the result ZIP file to make it smaller.
//!
//! A mcchanges pack can also hold the assets of many game versions in a compact way. Such a
//! versioned mcchanges pack has a `mcchanges.json` file at its root with this structure:
//!
//! ```json
//! {
//!     "format": 1,
//!     "versions": ["1.8.9", "1.9", "1.9.4"],
//!     "removed": { "1.9": ["assets/minecraft/textures/blocks/some_deleted_texture.png"] }
//! }
//! ```
//!
//! `versions` lists the versions from the oldest to the newest. Every version has a directory
//! named after it that holds the files that the version added or changed relative to the
//! previous one, and the oldest version holds all of its files. `removed` lists, for each
//! version, the files that the version deleted relative to the previous one. Selecting a version
//! means looking for each file in the directory of that version first, and then in the
//! directories of the previous versions, stopping at the first version that has or removed it.
//!
//! The files of a version do not have to be stored. The index can list them with a hash token
//! instead (`hashed`), and a hashes-only mcchanges pack stores nothing but tokens. Tokens are
//! `algorithm:digest` texts. See [`hash_matches`] for the algorithms. Language files have a token
//! of the kind `langkeys:`, which refers to a `<path>.langkeys` file next to the version files. It
//! holds a hash for every language entry, so that identical entries can still be removed.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use spng::{ContextFlags, DecodeFlags, Format};
use thiserror::Error;

use crate::debug_log::detail;
use crate::vfs::{
	IteratorTraversalOptions, VfsPackFileIterEntry, VirtualFileSystem, read_file, read_to_end
};

pub(crate) use zip_reader::ZipArchive;
use zip_reader::ZipReadError;

mod zip_reader;

#[cfg(test)]
pub(crate) mod tests;

/// The name of the file that marks a mcchanges pack as versioned.
const VERSION_INDEX_FILE: &str = "mcchanges.json";

/// The PNG file signature, followed by the length and type of the IHDR chunk, which
/// always is the first chunk of a PNG file.
const PNG_HEADER: [u8; 16] = [
	0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R'
];

/// Images with more pixels than this are only compared byte by byte, to bound the
/// memory needed to decode them.
const MAX_PIXEL_COMPARISON_PIXELS: u64 = 4096 * 4096;

/// The length of the hash of a language entry in a `langkeys:` `.langkeys` file. 96 bits make
/// accidental collisions impossible for all practical purposes.
const LANGUAGE_KEY_HASH_LENGTH: usize = 12;

/// The length of the hash of a language entry in `langfull:` and `langdelta:` `.langkeys` files.
/// 64 bits are enough: the chance that a different entry has the same hash as one of the 10000
/// entries of a language file is smaller than one in a quadrillion.
const COMPACT_LANGUAGE_KEY_HASH_LENGTH: usize = 8;

/// How many `langdelta:` files can be applied on top of a `langfull:` one, at most.
const MAX_LANGUAGE_KEY_DELTAS: usize = 64;

/// The suffix of the files with the language entry hashes of a language file.
const LANGUAGE_KEYS_SUFFIX: &str = ".langkeys";

/// The hashes of the entries of a language file. A hash is the first `width` bytes of the SHA-256
/// digest of an entry, read as a number.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LanguageKeySet {
	width: usize,
	hashes: HashSet<u128>
}

impl LanguageKeySet {
	/// Checks whether the language file has an entry with the given key and value.
	fn contains(&self, key: &str, value: &[u8]) -> bool {
		self.hashes
			.contains(&language_key_hash(key, value, self.width))
	}
}

/// An error that may occur while loading a mcchanges pack.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum McChangesError {
	/// Thrown when an I/O error occurs while reading the mcchanges pack.
	#[error("I/O error: {0}")]
	IoError(#[from] io::Error),
	/// Thrown when the file or directory that `mcchanges_path` points to can't be read.
	#[error(
		"mcchanges_path {path} can't be read: {source}. Check that the path is correct and that it exists"
	)]
	Unreadable {
		/// The path of the mcchanges pack.
		path: String,
		/// The error that happened.
		source: io::Error
	},
	/// Thrown when the mcchanges pack is a file, but not a valid ZIP archive.
	#[error("the mcchanges pack is neither a directory nor a valid ZIP file: {0}")]
	InvalidArchive(#[from] ZipReadError),
	/// Thrown when the version index of a versioned mcchanges pack is not valid.
	#[error("the mcchanges pack version index is not valid: {0}")]
	InvalidVersionIndex(String),
	/// Thrown when the mcchanges pack holds several game versions, but none was selected.
	#[error(
		"the mcchanges pack holds several game versions, so mcchanges_version must be set to one of: {0}"
	)]
	VersionRequired(String),
	/// Thrown when the selected game version is not in the mcchanges pack.
	#[error("the mcchanges pack does not hold the game version {version}. It holds: {available}")]
	UnknownVersion {
		/// The version that was selected.
		version: String,
		/// The comma-separated versions that the mcchanges pack holds.
		available: String
	},
	/// Thrown when a game version was selected, but the mcchanges pack is not versioned. The
	/// message tells what the mcchanges pack is, and what to do about it.
	#[error("mcchanges_version is set, but {0}")]
	NotVersioned(String)
}

enum McChangesSource {
	/// A directory in the virtual filesystem that holds the mcchanges pack files.
	Directory(PathBuf),
	/// A ZIP archive (e.g. a vanilla client JAR) that was loaded in memory.
	Archive(ZipArchive)
}

/// The contents of the `mcchanges.json` file.
#[derive(Deserialize)]
struct VersionIndexFile {
	format: u32,
	versions: Vec<String>,
	#[serde(default)]
	removed: HashMap<String, Vec<String>>,
	/// For each version, the files that it added or changed, but whose contents are not
	/// stored in the mcchanges pack. They are represented by a hash in the format
	/// `algorithm:hexadecimal digest`, with the algorithms `sha1` and `sha256`.
	#[serde(default)]
	hashed: HashMap<String, HashMap<String, String>>
}

/// A game version of a versioned mcchanges pack, and the files that it deleted
/// relative to the previous version.
struct VersionLayer {
	version: String,
	removed_files: HashSet<String>,
	hashed_files: HashMap<String, String>
}

/// A file of an mcchanges pack.
enum McChangesEntry {
	/// The contents of the file.
	Data(Vec<u8>),
	/// The hash of the contents of the file, as `algorithm:hexadecimal digest`.
	Hash(String),
	/// The hashes of the entries of a language file. The file itself is not known.
	LanguageKeys(LanguageKeySet)
}

/// The exact name of the mcchanges jar for Java Edition in the downloads of the mc-changes repository.
const JAVA_JAR_NAME: &str = "mcchanges-java.jar";
/// The exact name of the mcchanges jar for Bedrock Edition in the downloads of the mc-changes repository.
const BEDROCK_JAR_NAME: &str = "mcchanges-bedrock.jar";

/// Finds the mcchanges jar in a folder. If `path` is a directory that is not a mcchanges pack itself (it
/// has no `mcchanges.json` file at its root), and it has a file with the exact name of the mcchanges jar of
/// the edition of the pack that holds a `mcchanges.json` file, the path of that file and a message that tells
/// about it are returned. Otherwise, there is nothing to find, and the path must be used as it is.
pub(crate) async fn find_mcchanges_jar_in_directory<V: VirtualFileSystem>(
	vfs: &V,
	path: &Path,
	is_bedrock: bool
) -> Option<(PathBuf, String)> {
	if !vfs.file_type(path).ok()?.is_dir() || vfs.file_type(&path.join(VERSION_INDEX_FILE)).is_ok() {
		return None;
	}

	let jar_name = if is_bedrock {
		BEDROCK_JAR_NAME
	} else {
		JAVA_JAR_NAME
	};
	let jar_path = path.join(jar_name);

	if !vfs.file_type(&jar_path).ok()?.is_file() {
		return None;
	}

	let file = vfs.open(&jar_path).ok()?;
	let archive = ZipArchive::parse(
		read_to_end(file.file_read, file.file_size_hint)
			.await
			.ok()?
	)
	.ok()?;

	archive.contains(VERSION_INDEX_FILE).then(|| {
		let message = format!(
			"mcchanges_path is the folder {}, which is not a mcchanges pack, so PackSquash uses {jar_name} from it",
			path.display()
		);

		(jar_path, message)
	})
}

/// The result of choosing the game version of a versioned mcchanges pack automatically.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct VersionDetection {
	/// The chosen game version.
	pub(crate) version: String,
	/// How many pack files are identical to the files of the chosen version.
	pub(crate) identical_files: usize,
	/// How many versions were compared.
	pub(crate) versions_compared: usize,
	/// The other versions with as many identical files as the chosen one, from the newest to the oldest.
	pub(crate) tied_versions: Vec<String>,
	/// The best of the versions that have fewer identical files, and how many.
	pub(crate) next_best: Option<(String, usize)>
}

impl VersionDetection {
	/// Describes the choice for the user.
	pub(crate) fn describe(&self) -> String {
		if self.identical_files == 0 {
			return format!(
				"mcchanges_version is not set, so PackSquash compared the pack with the {} game versions of \
				the mcchanges pack. No pack file is identical to a file of any of them, so no file is left \
				out. The newest version, {}, was used",
				self.versions_compared, self.version
			);
		}

		let mut message = format!(
			"mcchanges_version is not set, so PackSquash compared the pack with the {} game versions of \
			the mcchanges pack and chose {}, which has {} identical pack files",
			self.versions_compared, self.version, self.identical_files
		);

		if !self.tied_versions.is_empty() {
			message.push_str(&format!(
				". {} other versions have as many ({}), and the newest of them was chosen",
				self.tied_versions.len(),
				self.tied_versions
					.iter()
					.take(5)
					.cloned()
					.collect::<Vec<_>>()
					.join(", ")
					+ if self.tied_versions.len() > 5 {
						", ..."
					} else {
						""
					}
			));
		}

		if let Some((version, count)) = &self.next_best {
			message.push_str(&format!(
				". The next best version is {version} with {count}"
			));
		}

		message.push_str(". To use another version, set mcchanges_version");

		message
	}
}

/// Tells whether a path is of a file that identifies the pack itself, and that must always be kept.
fn is_pack_identity_file(relative_path: &str) -> bool {
	matches!(
		relative_path,
		"pack.mcmeta" | "pack.png" | "manifest.json" | "pack_icon.png" | "bug_pack_icon.png"
	)
}

/// Tells whether a hash token is of a language file.
fn is_language_token(token: &str) -> bool {
	token.starts_with("langkeys:")
		|| token.starts_with("langfull:")
		|| token.starts_with("langdelta:")
}

/// A loaded mcchanges pack, ready to be used to find redundant pack files.
pub(crate) struct McChanges {
	source: McChangesSource,
	/// For versioned mcchanges packs, the layers of the selected version and all the
	/// previous ones, from the newest to the oldest.
	layers: Option<Vec<VersionLayer>>
}

/// The result of removing the language keys that a mcchanges pack already provides from
/// a language file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LanguageFileReduction {
	/// There is nothing to remove, or the file can't be safely reduced.
	Unchanged,
	/// Every key is already provided by the mcchanges pack, so the file is not needed.
	Redundant,
	/// Some keys were removed. The reduced file contents are provided.
	Reduced { data: Vec<u8> }
}

impl McChanges {
	/// Loads the mcchanges pack at the given path, which can be either a directory or a
	/// ZIP archive such as a vanilla client JAR. If the mcchanges pack holds the assets of
	/// several game versions, `version` selects which one is used.
	pub(crate) async fn open<V: VirtualFileSystem>(
		vfs: &V,
		path: &Path,
		version: Option<&str>
	) -> Result<Self, McChangesError> {
		let mut mcchanges = Self::open_source(vfs, path).await?;

		mcchanges.layers = match (mcchanges.read_raw(vfs, VERSION_INDEX_FILE).await, version) {
			(Some(index), version) => Some(Self::select_version(&index, version)?),
			(None, None) => None,
			(None, Some(_)) => return Err(mcchanges.not_versioned_error(vfs, path))
		};

		detail!(
			"mcchanges pack: {}",
			mcchanges.layers.as_ref().map_or_else(
				|| "it has no version index, so it is used as one set of files".to_string(),
				|layers| format!(
					"game version {} selected, which has {} layers from {} down to {}",
					layers.first().map_or("?", |layer| layer.version.as_str()),
					layers.len(),
					layers.first().map_or("?", |layer| layer.version.as_str()),
					layers.last().map_or("?", |layer| layer.version.as_str())
				)
			)
		);

		Ok(mcchanges)
	}

	/// Loads the mcchanges pack like [`Self::open`], but chooses the game version of a versioned
	/// mcchanges pack by itself: the version that has the most files identical to the files of the
	/// pack at `pack_directory`. If more than one version has the most, the newest of them is used.
	/// This is what is done when no game version is set. If `explicit` is set, the user asked for the
	/// automatic choice, so a mcchanges pack that is not versioned is an error. Otherwise, such a
	/// mcchanges pack is used as it is. The second value is `None` if no choice was made.
	pub(crate) async fn open_auto<V: VirtualFileSystem>(
		vfs: &V,
		path: &Path,
		pack_directory: &Path,
		traversal_options: IteratorTraversalOptions,
		explicit: bool
	) -> Result<(Self, Option<VersionDetection>), McChangesError> {
		let mut mcchanges = Self::open_source(vfs, path).await?;

		let Some(index) = mcchanges.read_raw(vfs, VERSION_INDEX_FILE).await else {
			return if explicit {
				Err(mcchanges.not_versioned_error(vfs, path))
			} else {
				Ok((mcchanges, None))
			};
		};

		let index = Self::parse_version_index(&index)?;

		if index.versions.is_empty() {
			return Err(McChangesError::InvalidVersionIndex(
				"the list of versions is empty".to_string()
			));
		}

		let pack_files = vfs
			.file_iterator(pack_directory, traversal_options)
			.filter_map(Result::ok)
			.collect::<Vec<_>>();
		let detection = mcchanges.detect_version(vfs, &index, &pack_files).await;
		let position = index
			.versions
			.iter()
			.position(|version| *version == detection.version)
			.expect("the detected version comes from the index");

		mcchanges.layers = Some(Self::version_layers(index, position));

		Ok((mcchanges, Some(detection)))
	}

	/// Loads the files of the mcchanges pack, without choosing a game version.
	async fn open_source<V: VirtualFileSystem>(vfs: &V, path: &Path) -> Result<Self, McChangesError> {
		let unreadable = |source: io::Error| McChangesError::Unreadable {
			path: path.display().to_string(),
			source
		};

		let source = if vfs.file_type(path).map_err(unreadable)?.is_dir() {
			McChangesSource::Directory(path.to_path_buf())
		} else {
			let file = vfs.open(path).map_err(unreadable)?;
			let archive_data = read_to_end(file.file_read, file.file_size_hint)
				.await
				.map_err(unreadable)?;

			McChangesSource::Archive(ZipArchive::parse(archive_data)?)
		};

		detail!(
			"mcchanges pack at {}: {}",
			path.display(),
			match &source {
				McChangesSource::Directory(_) => "a directory",
				McChangesSource::Archive(_) => "a ZIP archive"
			}
		);

		Ok(Self {
			source,
			layers: None
		})
	}

	/// Builds the error for a mcchanges pack that has no version index, but has a game version
	/// selected. The error says what the mcchanges pack looks like and how to fix the options,
	/// because this is a likely mistake when the wrong path is set.
	fn not_versioned_error<V: VirtualFileSystem>(&self, vfs: &V, path: &Path) -> McChangesError {
		let path = path.display();

		McChangesError::NotVersioned(match &self.source {
			McChangesSource::Directory(directory) => {
				let jar_names = Self::jar_names_in(vfs, directory);

				format!(
					"the folder {path} has no {VERSION_INDEX_FILE} file, so it is not a jar from \
					mc-changes. Remove mcchanges_version, or set mcchanges_path to a jar from \
					mc-changes, like mcchanges-java.jar, or to a folder that has it{}",
					if jar_names.is_empty() {
						String::new()
					} else {
						format!(
							". This folder has these jar files: {}. Set mcchanges_path to the right one",
							jar_names.join(", ")
						)
					}
				)
			}
			McChangesSource::Archive(archive) if archive.contains(VERSION_INDEX_FILE) => format!(
				"the file {path} has a {VERSION_INDEX_FILE} file inside, but PackSquash can't read \
				it. The jar may be damaged. Download it again"
			),
			McChangesSource::Archive(_) => format!(
				"the file {path} has no {VERSION_INDEX_FILE} file inside, so it is not a jar from \
				mc-changes. It may be a game jar. Remove mcchanges_version to use it as a game jar, \
				or set mcchanges_path to mcchanges-java.jar or mcchanges-bedrock.jar from \
				https://github.com/mocny-gaz/mc-changes/releases"
			)
		})
	}

	/// Lists the names of some of the jar files that are directly in a directory.
	fn jar_names_in<V: VirtualFileSystem>(vfs: &V, directory: &Path) -> Vec<String> {
		let mut names = vfs
			.file_iterator(directory, IteratorTraversalOptions::default())
			// Do not walk a huge tree to make an error message
			.take(5000)
			.filter_map(Result::ok)
			.filter_map(|entry| {
				let relative_path = entry.file_path.strip_prefix(directory).ok()?;

				(relative_path.components().count() == 1
					&& relative_path
						.extension()
						.is_some_and(|extension| extension.eq_ignore_ascii_case("jar")))
				.then(|| relative_path.to_string_lossy().into_owned())
			})
			.collect::<Vec<_>>();

		names.sort();
		names.truncate(10);
		names
	}

	/// Parses the version index.
	fn parse_version_index(index: &[u8]) -> Result<VersionIndexFile, McChangesError> {
		let index = serde_json::from_slice::<VersionIndexFile>(index)
			.map_err(|err| McChangesError::InvalidVersionIndex(err.to_string()))?;

		if index.format != 1 {
			return Err(McChangesError::InvalidVersionIndex(format!(
				"unsupported format {}",
				index.format
			)));
		}

		Ok(index)
	}

	/// Parses the version index and returns the layers of the selected version.
	fn select_version(
		index: &[u8],
		version: Option<&str>
	) -> Result<Vec<VersionLayer>, McChangesError> {
		let index = Self::parse_version_index(index)?;

		let available = index.versions.join(", ");
		let Some(version) = version else {
			return Err(McChangesError::VersionRequired(available));
		};
		let Some(selected_position) = index.versions.iter().position(|v| v == version) else {
			return Err(McChangesError::UnknownVersion {
				version: version.to_owned(),
				available
			});
		};

		Ok(Self::version_layers(index, selected_position))
	}

	/// Builds the layers of the version at the given position of the index, from the newest layer to
	/// the oldest.
	fn version_layers(index: VersionIndexFile, selected_position: usize) -> Vec<VersionLayer> {
		let mut removed = index.removed;
		let mut hashed = index.hashed;

		index.versions[..=selected_position]
			.iter()
			.rev()
			.map(|version| VersionLayer {
				version: version.clone(),
				removed_files: removed
					.remove(version)
					.map(|files| files.into_iter().collect())
					.unwrap_or_default(),
				hashed_files: hashed.remove(version).unwrap_or_default()
			})
			.collect()
	}

	/// Counts, for every game version of the index, how many of the pack files are identical to the
	/// file with the same path in that version, and picks the version with the most. The newest wins
	/// a tie. Pack files that identify the pack and language files are not counted, because they
	/// do not tell the game version.
	///
	/// A path can only change in the versions that mention it, so the pack file is compared once
	/// with each of those, and the result counts for every version until the next one that mentions
	/// the path. This makes the work almost independent of the number of versions.
	async fn detect_version<V: VirtualFileSystem>(
		&self,
		vfs: &V,
		index: &VersionIndexFile,
		pack_files: &[VfsPackFileIterEntry]
	) -> VersionDetection {
		/// What a version says about a path.
		enum Definition<'a> {
			Data(Vec<u8>),
			Hash(&'a str),
			/// The version deletes the file, or it is something that is not compared.
			NoMatch
		}

		let version_count = index.versions.len();
		let no_hashes = HashMap::new();
		let removed = index
			.versions
			.iter()
			.map(|version| {
				index
					.removed
					.get(version)
					.map(|files| files.iter().map(String::as_str).collect::<HashSet<_>>())
					.unwrap_or_default()
			})
			.collect::<Vec<_>>();
		let hashed = index
			.versions
			.iter()
			.map(|version| index.hashed.get(version).unwrap_or(&no_hashes))
			.collect::<Vec<_>>();

		// Differences between the count of a version and the one before it
		let mut changes = vec![0_i64; version_count + 1];

		for pack_file in pack_files {
			let path = pack_file.relative_path.as_str();

			if is_pack_identity_file(path) || path.contains("/lang/") {
				continue;
			}

			let mut definitions = Vec::new();

			for (position, version) in index.versions.iter().enumerate() {
				if let Some(data) = self.read_raw(vfs, &format!("{version}/{path}")).await {
					definitions.push((position, Definition::Data(data)));
				} else if let Some(token) = hashed[position].get(path) {
					definitions.push((
						position,
						if is_language_token(token) {
							Definition::NoMatch
						} else {
							Definition::Hash(token)
						}
					));
				} else if removed[position].contains(path) {
					definitions.push((position, Definition::NoMatch));
				}
			}

			if definitions
				.iter()
				.all(|(_, definition)| matches!(definition, Definition::NoMatch))
			{
				continue;
			}

			let Some(pack_data) = read_file(vfs, &pack_file.file_path).await else {
				continue;
			};

			for (definition_index, (position, definition)) in definitions.iter().enumerate() {
				let identical = match definition {
					Definition::Data(data) if path.ends_with(".png") => {
						png_is_identical(&pack_data, data)
					}
					Definition::Data(data) if path.ends_with(".json") => {
						json_is_identical(&pack_data, data)
					}
					Definition::Data(data) => pack_data == *data,
					Definition::Hash(token) => hash_matches(&pack_data, token),
					Definition::NoMatch => false
				};

				if identical {
					let end = definitions
						.get(definition_index + 1)
						.map_or(version_count, |(next_position, _)| *next_position);

					changes[*position] += 1;
					changes[end] -= 1;
				}
			}
		}

		let mut identical_files = Vec::with_capacity(version_count);
		let mut running_total = 0_i64;

		for change in &changes[..version_count] {
			running_total += change;
			identical_files.push(running_total as usize);
		}

		detail!(
			"Automatic game version choice, pack files identical to each version of the mcchanges pack: {}",
			index
				.versions
				.iter()
				.zip(&identical_files)
				.map(|(version, count)| format!("{version}={count}"))
				.collect::<Vec<_>>()
				.join(", ")
		);

		let best_count = identical_files.iter().copied().max().unwrap_or(0);
		let best_position = identical_files
			.iter()
			.rposition(|count| *count == best_count)
			.expect("an index with versions has a best version");

		VersionDetection {
			version: index.versions[best_position].clone(),
			identical_files: best_count,
			versions_compared: version_count,
			tied_versions: identical_files
				.iter()
				.enumerate()
				.rev()
				.filter(|(position, count)| **count == best_count && *position != best_position)
				.map(|(position, _)| index.versions[position].clone())
				.collect(),
			next_best: identical_files
				.iter()
				.enumerate()
				.rev()
				.filter(|(_, count)| **count < best_count)
				.max_by_key(|(position, count)| (**count, *position))
				.map(|(position, count)| (index.versions[position].clone(), *count))
		}
	}

	/// Reads a file at the given path, relative to the root of the mcchanges pack, if it
	/// exists and can be read.
	async fn read_raw<V: VirtualFileSystem>(&self, vfs: &V, path: &str) -> Option<Vec<u8>> {
		match &self.source {
			McChangesSource::Directory(directory) => read_file(vfs, &directory.join(path)).await,
			McChangesSource::Archive(archive) => archive.read(path)
		}
	}

	/// Finds the mcchanges pack file with the given relative path. For versioned mcchanges
	/// packs, this finds the file as it is in the selected game version, which can be known only
	/// by its hash. Returns `None` if the file does not exist or can't be read.
	async fn lookup<V: VirtualFileSystem>(
		&self,
		vfs: &V,
		relative_path: &str
	) -> Option<McChangesEntry> {
		let Some(layers) = &self.layers else {
			let entry = self
				.read_raw(vfs, relative_path)
				.await
				.map(McChangesEntry::Data);

			detail!(
				"mcchanges: {relative_path}: {}",
				match &entry {
					Some(McChangesEntry::Data(data)) =>
						format!("found as a data file of {} bytes", data.len()),
					_ => "not in the mcchanges pack".to_string()
				}
			);

			return entry;
		};

		for (index, layer) in layers.iter().enumerate() {
			if let Some(data) = self
				.read_raw(vfs, &format!("{}/{relative_path}", layer.version))
				.await
			{
				detail!(
					"mcchanges: {relative_path}: found as a data file of {} bytes in the layer of the game version {}",
					data.len(),
					layer.version
				);

				return Some(McChangesEntry::Data(data));
			}

			if let Some(hash) = layer.hashed_files.get(relative_path) {
				detail!(
					"mcchanges: {relative_path}: found as the hash token {hash} in the layer of the game version {}",
					layer.version
				);

				if is_language_token(hash) {
					// A language file whose keys can't be loaded is left as an unmatchable hash
					return Some(
						self.load_language_keys(vfs, layers, index, relative_path)
							.await
							.map_or_else(
								|| McChangesEntry::Hash(hash.clone()),
								McChangesEntry::LanguageKeys
							)
					);
				}

				return Some(McChangesEntry::Hash(hash.clone()));
			}

			if layer.removed_files.contains(relative_path) {
				detail!(
					"mcchanges: {relative_path}: deleted by the layer of the game version {}, so it does not exist in the selected version",
					layer.version
				);

				return None;
			}
		}

		detail!(
			"mcchanges: {relative_path}: not in any layer from the game version {} to {}",
			layers.first().map_or("?", |layer| layer.version.as_str()),
			layers.last().map_or("?", |layer| layer.version.as_str())
		);

		None
	}

	/// Loads the hashes of the entries of a language file whose token is in the layer with the
	/// given index. A `langdelta:` token only has the changes from the previous version of the file,
	/// so this follows the older layers that mention the file until a `langfull:` (or an old
	/// `langkeys:`) token, then applies the changes in order. Every `.langkeys` file must match the
	/// digest of its token.
	async fn load_language_keys<V: VirtualFileSystem>(
		&self,
		vfs: &V,
		layers: &[VersionLayer],
		start: usize,
		relative_path: &str
	) -> Option<LanguageKeySet> {
		let mut index = start;
		let mut deltas = Vec::new();

		loop {
			let layer = &layers[index];
			let (kind, digest) = layer.hashed_files.get(relative_path)?.split_once(':')?;
			let data = self
				.read_raw(
					vfs,
					&format!("{}/{relative_path}{LANGUAGE_KEYS_SUFFIX}", layer.version)
				)
				.await?;

			if !hexadecimal(Sha256::digest(&data)).eq_ignore_ascii_case(digest) {
				return None;
			}

			match kind {
				"langkeys" if deltas.is_empty() => {
					return Some(LanguageKeySet {
						width: LANGUAGE_KEY_HASH_LENGTH,
						hashes: parse_hash_list(&data, LANGUAGE_KEY_HASH_LENGTH)?
							.into_iter()
							.collect()
					});
				}
				"langfull" => {
					let mut hashes: HashSet<u128> =
						parse_hash_list(&data, COMPACT_LANGUAGE_KEY_HASH_LENGTH)?
							.into_iter()
							.collect();

					for (added, removed) in deltas.iter().rev() {
						for hash in removed {
							hashes.remove(hash);
						}
						hashes.extend(added);
					}

					return Some(LanguageKeySet {
						width: COMPACT_LANGUAGE_KEY_HASH_LENGTH,
						hashes
					});
				}
				"langdelta" if deltas.len() < MAX_LANGUAGE_KEY_DELTAS => {
					deltas.push(parse_language_key_delta(&data)?);
				}
				_ => return None
			}

			// The previous change of the file is in the next older version that mentions it
			index = layers
				.iter()
				.enumerate()
				.skip(index + 1)
				.find(|(_, layer)| {
					layer.hashed_files.contains_key(relative_path)
						|| layer.removed_files.contains(relative_path)
				})
				.map(|(index, _)| index)?;
		}
	}

	/// Checks whether including the given pack file in the result ZIP file is redundant
	/// because the mcchanges pack already provides the same data under the same path.
	///
	/// Every doubt is resolved by considering the file not redundant, so files are
	/// never wrongly dropped: files that cannot be read, or that differ in any way
	/// that may be observable in the game, are not redundant.
	pub(crate) async fn is_redundant<V: VirtualFileSystem>(
		&self,
		vfs: &V,
		pack_file: &VfsPackFileIterEntry
	) -> bool {
		let relative_path = pack_file.relative_path.as_str();

		// The pack metadata, manifest and icons identify the pack itself, and must always be kept
		if is_pack_identity_file(relative_path) {
			detail!("mcchanges: {relative_path}: identifies the pack, so it is always kept");

			return false;
		}

		// Minecraft reads the animation metadata of a texture from the same pack that
		// provides the texture. Therefore, textures and their metadata can only be
		// dropped as a whole: dropping just one of them would change the result
		if let Some(texture_relative_path) = relative_path
			.strip_suffix(".mcmeta")
			.filter(|texture_path| texture_path.ends_with(".png"))
		{
			return match pack_file
				.file_path
				.to_str()
				.and_then(|path| path.strip_suffix(".mcmeta"))
			{
				Some(texture_file_path) => {
					self.is_texture_redundant(
						vfs,
						texture_relative_path,
						Path::new(texture_file_path)
					)
					.await
				}
				None => false
			};
		}

		if relative_path.ends_with(".png") {
			return self
				.is_texture_redundant(vfs, relative_path, &pack_file.file_path)
				.await;
		}

		let (Some(pack_data), Some(mcchanges_entry)) = (
			read_file(vfs, &pack_file.file_path).await,
			self.lookup(vfs, relative_path).await
		) else {
			detail!(
				"mcchanges: {relative_path}: kept, because it is not in the mcchanges pack or one of them can't be read"
			);

			return false;
		};

		let redundant = match &mcchanges_entry {
			McChangesEntry::Hash(hash) => hash_matches(&pack_data, hash),
			McChangesEntry::Data(mcchanges_data) if relative_path.ends_with(".json") => {
				json_is_identical(&pack_data, mcchanges_data)
			}
			McChangesEntry::Data(mcchanges_data) => &pack_data == mcchanges_data,
			// Identical language files are left out by the language key removal instead
			McChangesEntry::LanguageKeys(_) => false
		};

		detail!(
			"mcchanges: {relative_path}: {}. {}",
			if redundant {
				"left out, it is the same as in the mcchanges pack"
			} else {
				"kept, it is different"
			},
			match &mcchanges_entry {
				McChangesEntry::Hash(hash) => describe_hash_comparison(&pack_data, hash),
				McChangesEntry::Data(data) =>
					describe_data_comparison(relative_path, &pack_data, data),
				McChangesEntry::LanguageKeys(_) =>
					"language file, handled by the removal of language keys".to_string()
			}
		);

		redundant
	}

	/// Checks whether a texture, along with its animation metadata, is the same as the
	/// mcchanges one.
	async fn is_texture_redundant<V: VirtualFileSystem>(
		&self,
		vfs: &V,
		texture_relative_path: &str,
		texture_file_path: &Path
	) -> bool {
		let (Some(pack_texture), Some(mcchanges_texture)) = (
			read_file(vfs, texture_file_path).await,
			self.lookup(vfs, texture_relative_path).await
		) else {
			return false;
		};

		let texture_is_identical = match &mcchanges_texture {
			McChangesEntry::Data(data) => png_is_identical(&pack_texture, data),
			McChangesEntry::Hash(hash) => hash_matches(&pack_texture, hash),
			McChangesEntry::LanguageKeys(_) => false
		};

		detail!(
			"mcchanges: {texture_relative_path}: the texture is {}. {}",
			if texture_is_identical {
				"the same pixels"
			} else {
				"different, so the texture and its metadata are kept"
			},
			match &mcchanges_texture {
				McChangesEntry::Hash(hash) => describe_hash_comparison(&pack_texture, hash),
				McChangesEntry::Data(data) =>
					describe_data_comparison(texture_relative_path, &pack_texture, data),
				McChangesEntry::LanguageKeys(_) => "it is a language file token".to_string()
			}
		);

		if !texture_is_identical {
			return false;
		}

		let mut metadata_file_path = OsString::from(texture_file_path);
		metadata_file_path.push(".mcmeta");

		let redundant = match (
			read_file(vfs, Path::new(&metadata_file_path)).await,
			self.lookup(vfs, &format!("{texture_relative_path}.mcmeta"))
				.await
		) {
			(None, None) => true,
			(Some(pack_metadata), Some(McChangesEntry::Data(mcchanges_metadata))) => {
				json_is_identical(&pack_metadata, &mcchanges_metadata)
			}
			(Some(pack_metadata), Some(McChangesEntry::Hash(hash))) => {
				hash_matches(&pack_metadata, &hash)
			}
			_ => false
		};

		detail!(
			"mcchanges: {texture_relative_path}: with its animation metadata, the texture is {}",
			if redundant {
				"left out, because the metadata is the same, or neither has any"
			} else {
				"kept, because only one of the pack and the mcchanges pack has animation metadata, or it is different"
			}
		);

		redundant
	}

	/// Removes the keys of a language file that have the same value in the language file
	/// of the mcchanges pack with the same path. As the game falls back to the mcchanges
	/// pack for the missing keys, the translations the player sees are the same.
	///
	/// Files that are not language files, that do not have a counterpart in the mcchanges pack,
	/// or that can't be parsed are reported as unchanged.
	pub(crate) async fn reduce_language_file<V: VirtualFileSystem>(
		&self,
		vfs: &V,
		pack_file: &VfsPackFileIterEntry
	) -> LanguageFileReduction {
		let relative_path = pack_file.relative_path.as_str();

		let mut components = relative_path.split('/');
		let is_in_language_directory = components.next() == Some("assets")
			&& components.next().is_some()
			&& components.next() == Some("lang")
			&& components.next().is_some();

		let is_json = if !is_in_language_directory {
			return LanguageFileReduction::Unchanged;
		} else if relative_path.ends_with(".json") {
			true
		} else if relative_path.ends_with(".lang") {
			false
		} else {
			return LanguageFileReduction::Unchanged;
		};

		let (Some(pack_data), Some(mcchanges_entry)) = (
			read_file(vfs, &pack_file.file_path).await,
			self.lookup(vfs, relative_path).await
		) else {
			return LanguageFileReduction::Unchanged;
		};

		match (mcchanges_entry, is_json) {
			(McChangesEntry::Data(mcchanges_data), true) => {
				let Some(mcchanges_entries) = parse_json_language_file(&mcchanges_data) else {
					return LanguageFileReduction::Unchanged;
				};

				reduce_json_language_file(&pack_data, |key, value| {
					mcchanges_entries.get(key) == Some(value)
				})
			}
			(McChangesEntry::Data(mcchanges_data), false) => {
				let Ok(mcchanges_data) = std::str::from_utf8(&mcchanges_data) else {
					return LanguageFileReduction::Unchanged;
				};
				let mcchanges_values = legacy_language_entries(mcchanges_data);

				reduce_legacy_language_file(&pack_data, |key, value| {
					mcchanges_values.get(key) == Some(&value)
				})
			}
			(McChangesEntry::LanguageKeys(keys), true) => {
				reduce_json_language_file(&pack_data, |key, value| {
					keys.contains(key, canonical_json(value).as_bytes())
				})
			}
			(McChangesEntry::LanguageKeys(keys), false) => {
				reduce_legacy_language_file(&pack_data, |key, value| {
					keys.contains(key, value.as_bytes())
				})
			}
			(McChangesEntry::Hash(_), _) => LanguageFileReduction::Unchanged
		}
	}
}

/// Formats a digest as lowercase hexadecimal.
fn hexadecimal(digest: impl AsRef<[u8]>) -> String {
	digest
		.as_ref()
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect()
}

/// Describes how the data of a pack file compares with a hash token of the mcchanges pack, for the debug
/// log: the token of the mcchanges pack, the token that the pack file has for the same algorithm, and the
/// result. The algorithm and the digest are what the build script of the mcchanges pack wrote.
fn describe_hash_comparison(data: &[u8], token: &str) -> String {
	let algorithm = token.split_once(':').map_or("", |(algorithm, _)| algorithm);
	let pack_token = match algorithm {
		"sha1" => format!("sha1:{}", hexadecimal(Sha1::digest(data))),
		"sha256" => format!("sha256:{}", hexadecimal(Sha256::digest(data))),
		"json" => parse_json(data).map_or_else(
			|| "the pack file is not JSON that can be read".to_string(),
			|value| {
				format!(
					"json:{}",
					hexadecimal(Sha256::digest(canonical_json(&value)))
				)
			}
		),
		"png" => match (png_size(data), decode_png(data)) {
			(Some((width, height)), Some(pixels)) => {
				format!(
					"png:{width}x{height}:{}",
					hexadecimal(Sha256::digest(&pixels))
				)
			}
			_ => "the pack file is not a PNG image that can be decoded".to_string()
		},
		_ => "the algorithm of the token is not known".to_string()
	};

	format!(
		"mcchanges token {token}, pack file token {pack_token} => {}",
		if hash_matches(data, token) {
			"identical"
		} else {
			"different"
		}
	)
}

/// Describes how the data of a pack file compares with the data file of the mcchanges pack, for the debug log.
fn describe_data_comparison(relative_path: &str, pack_data: &[u8], mcchanges_data: &[u8]) -> String {
	let (method, identical) = if relative_path.ends_with(".png") {
		("PNG pixels", png_is_identical(pack_data, mcchanges_data))
	} else if relative_path.ends_with(".json") {
		(
			"canonical JSON",
			json_is_identical(pack_data, mcchanges_data)
		)
	} else {
		("bytes", pack_data == mcchanges_data)
	};

	format!(
		"mcchanges data file of {} bytes, pack file of {} bytes, compared as {method} => {}",
		mcchanges_data.len(),
		pack_data.len(),
		if identical { "identical" } else { "different" }
	)
}

/// Checks whether some data matches a token, in the format `algorithm:digest`. The algorithms are:
///
/// - `sha1` and `sha256`: the digest of the bytes.
/// - `json`: the SHA-256 digest of [`canonical_json`] of the parsed data.
/// - `png`: `<width>x<height>:<digest>`, the SHA-256 digest of the RGBA8 pixels that [`decode_png`]
///   gives. This is how [`png_is_identical`] compares textures.
///
/// Unknown algorithms never match.
fn hash_matches(data: &[u8], hash: &str) -> bool {
	let Some((algorithm, expected_digest)) = hash.split_once(':') else {
		return false;
	};

	match algorithm {
		"sha1" => hexadecimal(Sha1::digest(data)).eq_ignore_ascii_case(expected_digest),
		"sha256" => hexadecimal(Sha256::digest(data)).eq_ignore_ascii_case(expected_digest),
		"json" => parse_json(data).is_some_and(|value| {
			hexadecimal(Sha256::digest(canonical_json(&value))).eq_ignore_ascii_case(expected_digest)
		}),
		"png" => png_token_matches(data, expected_digest),
		_ => false
	}
}

/// Checks whether a PNG image matches a `png` token without its algorithm.
fn png_token_matches(png: &[u8], token: &str) -> bool {
	let Some((size, expected_digest)) = token.split_once(':') else {
		return false;
	};
	let Some((width, height)) = size
		.split_once('x')
		.and_then(|(width, height)| Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?)))
	else {
		return false;
	};

	if png_size(png) != Some((width, height))
		|| u64::from(width) * u64::from(height) > MAX_PIXEL_COMPARISON_PIXELS
	{
		return false;
	}

	decode_png(png).is_some_and(|pixels| {
		hexadecimal(Sha256::digest(&pixels)).eq_ignore_ascii_case(expected_digest)
	})
}

/// Parses JSON data like the game does for the files that matter here: UTF-8, with a BOM
/// being ignored.
fn parse_json(data: &[u8]) -> Option<serde_json::Value> {
	serde_json::from_slice(strip_bom(data)).ok()
}

/// Writes JSON in a canonical form: no whitespace, and object keys sorted by their UTF-8 bytes.
/// Documents with the same contents have the same canonical form, and the build script of
/// mcchanges jars writes the same text.
fn canonical_json(value: &serde_json::Value) -> String {
	fn write(value: &serde_json::Value, out: &mut String) {
		match value {
			serde_json::Value::Array(items) => {
				out.push('[');
				for (index, item) in items.iter().enumerate() {
					if index > 0 {
						out.push(',');
					}
					write(item, out);
				}
				out.push(']');
			}
			serde_json::Value::Object(entries) => {
				let mut entries: Vec<_> = entries.iter().collect();
				entries.sort_unstable_by(|a, b| a.0.cmp(b.0));

				out.push('{');
				for (index, (key, item)) in entries.into_iter().enumerate() {
					if index > 0 {
						out.push(',');
					}
					out.push_str(&serde_json::Value::from(key.as_str()).to_string());
					out.push(':');
					write(item, out);
				}
				out.push('}');
			}
			// The compact form of a scalar JSON value is its canonical form
			scalar => out.push_str(&scalar.to_string())
		}
	}

	let mut out = String::new();
	write(value, &mut out);
	out
}

/// The hash of an entry of a language file, as stored in `.langkeys` files: the first `width` bytes
/// of the SHA-256 digest of the key, a zero byte, and the value, read as a big endian number. The
/// value is the canonical JSON of the value for JSON language files, and the text after the equals
/// sign for legacy ones.
fn language_key_hash(key: &str, value: &[u8], width: usize) -> u128 {
	let mut hasher = Sha256::new();
	hasher.update(key.as_bytes());
	hasher.update([0]);
	hasher.update(value);

	let digest = hasher.finalize();
	let digest: &[u8] = digest.as_ref();

	digest[..width]
		.iter()
		.fold(0, |hash, byte| (hash << 8) | u128::from(*byte))
}

/// Parses hashes of `width` bytes that are stored one after the other. Fails if the length of the
/// data is not a multiple of `width`.
fn parse_hash_list(data: &[u8], width: usize) -> Option<Vec<u128>> {
	if !data.len().is_multiple_of(width) {
		return None;
	}

	Some(
		data.chunks_exact(width)
			.map(|hash| {
				hash.iter()
					.fold(0, |hash, byte| (hash << 8) | u128::from(*byte))
			})
			.collect()
	)
}

/// Parses a `langdelta:` file: the number of added hashes as a little endian 32 bit number, the added
/// hashes, and then the removed hashes, which are the rest of the file.
fn parse_language_key_delta(data: &[u8]) -> Option<(Vec<u128>, Vec<u128>)> {
	let (count, hashes) = data.split_first_chunk::<4>()?;
	let added_length = usize::try_from(u32::from_le_bytes(*count))
		.ok()?
		.checked_mul(COMPACT_LANGUAGE_KEY_HASH_LENGTH)?;
	let (added, removed) = hashes.split_at_checked(added_length)?;

	Some((
		parse_hash_list(added, COMPACT_LANGUAGE_KEY_HASH_LENGTH)?,
		parse_hash_list(removed, COMPACT_LANGUAGE_KEY_HASH_LENGTH)?
	))
}

fn strip_bom(data: &[u8]) -> &[u8] {
	data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data)
}

/// Checks whether two JSON documents have the same contents, regardless of how they are
/// formatted or the order of their object keys. If any of them is not valid JSON, they
/// are compared byte by byte.
fn json_is_identical(a: &[u8], b: &[u8]) -> bool {
	if a == b {
		return true;
	}

	matches!((parse_json(a), parse_json(b)), (Some(a), Some(b)) if a == b)
}

/// Parses a JSON language file, which has to be a JSON object.
fn parse_json_language_file(data: &[u8]) -> Option<serde_json::Map<String, serde_json::Value>> {
	match parse_json(data) {
		Some(serde_json::Value::Object(entries)) => Some(entries),
		_ => None
	}
}

/// Removes the entries of a JSON language file that the mcchanges language file has with the
/// same value, which `is_mcchanges_entry` tells for a key and a value.
fn reduce_json_language_file(
	pack: &[u8],
	is_mcchanges_entry: impl Fn(&str, &serde_json::Value) -> bool
) -> LanguageFileReduction {
	let Some(mut pack_entries) = parse_json_language_file(pack) else {
		return LanguageFileReduction::Unchanged;
	};

	let entry_count = pack_entries.len();
	pack_entries.retain(|key, value| !is_mcchanges_entry(key, value));

	if pack_entries.len() == entry_count {
		LanguageFileReduction::Unchanged
	} else if pack_entries.is_empty() {
		LanguageFileReduction::Redundant
	} else {
		serde_json::to_vec(&pack_entries).map_or(LanguageFileReduction::Unchanged, |data| {
			LanguageFileReduction::Reduced { data }
		})
	}
}

/// Gets the key and value of a line of a legacy language file. Like the game does, this
/// ignores blank lines, comments, and lines without an equals sign, and does not trim anything
/// but the line terminator.
fn legacy_language_entry(line: &str) -> Option<(&str, &str)> {
	let line = line.trim_end_matches(['\r', '\n']);

	if line.is_empty() || line.starts_with('#') {
		None
	} else {
		line.split_once('=')
	}
}

/// Gets the entries of a legacy language file. When a key appears several times, the game
/// uses the last value, so that is the one that is returned.
fn legacy_language_entries(data: &str) -> HashMap<&str, &str> {
	data.split_inclusive('\n')
		.filter_map(legacy_language_entry)
		.collect()
}

/// Removes the entries of a legacy language file (`key=value` lines) that the mcchanges language
/// file has with the same value, which `is_mcchanges_entry` tells for a key and the value the
/// game uses for it. Comments, blank lines and line endings are kept.
fn reduce_legacy_language_file(
	pack: &[u8],
	is_mcchanges_entry: impl Fn(&str, &str) -> bool
) -> LanguageFileReduction {
	let Ok(pack) = std::str::from_utf8(pack) else {
		return LanguageFileReduction::Unchanged;
	};

	// A key is only removed if the value the game would use is provided by the mcchanges, which
	// means removing every line of that key
	let pack_values = legacy_language_entries(pack);

	let mut reduced = String::with_capacity(pack.len());
	let (mut removed_entries, mut kept_entries) = (0_usize, 0_usize);

	for line in pack.split_inclusive('\n') {
		match legacy_language_entry(line) {
			Some((key, _))
				if pack_values
					.get(key)
					.is_some_and(|value| is_mcchanges_entry(key, value)) =>
			{
				removed_entries += 1;
			}
			Some(_) => {
				kept_entries += 1;
				reduced.push_str(line);
			}
			None => reduced.push_str(line)
		}
	}

	if removed_entries == 0 {
		LanguageFileReduction::Unchanged
	} else if kept_entries == 0 {
		LanguageFileReduction::Redundant
	} else {
		LanguageFileReduction::Reduced {
			data: reduced.into_bytes()
		}
	}
}

/// Checks whether two PNG images look the same in the game: they have the same size and
/// pixels, regardless of how they were encoded. The color of fully transparent pixels is
/// ignored, as it can't be seen. If either image can't be decoded, or it is too big to
/// decode cheaply, they are compared byte by byte.
fn png_is_identical(a: &[u8], b: &[u8]) -> bool {
	if a == b {
		return true;
	}

	// Compare the sizes in the IHDR chunk before doing any costly decoding
	let (Some(size_a), Some(size_b)) = (png_size(a), png_size(b)) else {
		return false;
	};

	if size_a != size_b || u64::from(size_a.0) * u64::from(size_a.1) > MAX_PIXEL_COMPARISON_PIXELS {
		return false;
	}

	matches!((decode_png(a), decode_png(b)), (Some(a), Some(b)) if a == b)
}

/// Reads the width and height of a PNG image from its header, without decoding it.
fn png_size(png: &[u8]) -> Option<(u32, u32)> {
	if png.get(..PNG_HEADER.len())? != PNG_HEADER {
		return None;
	}

	Some((
		u32::from_be_bytes(png.get(16..20)?.try_into().ok()?),
		u32::from_be_bytes(png.get(20..24)?.try_into().ok()?)
	))
}

/// Decodes a PNG image to RGBA8 pixels, zeroing the color of fully transparent ones.
fn decode_png(png: &[u8]) -> Option<Vec<u8>> {
	let mut reader = spng::Decoder::new(png)
		.with_decode_flags(DecodeFlags::TRANSPARENCY)
		.with_context_flags(ContextFlags::IGNORE_ADLER32)
		.with_output_format(Format::Rgba8)
		.read_info()
		.ok()?;

	let mut pixels = vec![0; reader.output_buffer_size()];
	reader.next_frame(&mut pixels).ok()?;

	for pixel in pixels.as_chunks_mut::<4>().0 {
		if pixel[3] == 0 {
			pixel[..3].fill(0);
		}
	}

	Some(pixels)
}
