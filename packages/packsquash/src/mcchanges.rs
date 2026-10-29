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

use crate::vfs::{VfsPackFileIterEntry, VirtualFileSystem, read_file, read_to_end};

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

/// The length of the hash of a language entry in a `.langkeys` file. 96 bits make accidental
/// collisions impossible for all practical purposes.
const LANGUAGE_KEY_HASH_LENGTH: usize = 12;

/// The suffix of the files with the language entry hashes of a language file.
const LANGUAGE_KEYS_SUFFIX: &str = ".langkeys";

/// The hash of a language entry: a key and its value.
type LanguageKeyHash = [u8; LANGUAGE_KEY_HASH_LENGTH];

/// An error that may occur while loading a mcchanges pack.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum McChangesError {
	/// Thrown when an I/O error occurs while reading the mcchanges pack.
	#[error("I/O error: {0}")]
	IoError(#[from] io::Error),
	/// Thrown when the mcchanges pack is a file, but not a valid ZIP archive.
	#[error("the mcchanges pack is neither a directory nor a valid ZIP file: {0}")]
	InvalidArchive(#[from] ZipReadError),
	/// Thrown when the version index of a versioned mcchanges pack is not valid.
	#[error("the mcchanges pack version index is not valid: {0}")]
	InvalidVersionIndex(String),
	/// Thrown when the mcchanges pack holds several game versions, but none was selected.
	#[error("the mcchanges pack holds several game versions, so mcchanges_version must be set to one of: {0}")]
	VersionRequired(String),
	/// Thrown when the selected game version is not in the mcchanges pack.
	#[error("the mcchanges pack does not hold the game version {version}. It holds: {available}")]
	UnknownVersion {
		/// The version that was selected.
		version: String,
		/// The comma-separated versions that the mcchanges pack holds.
		available: String
	},
	/// Thrown when a game version was selected, but the mcchanges pack is not versioned.
	#[error("mcchanges_version is set, but the mcchanges pack does not hold several game versions")]
	NotVersioned
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
	LanguageKeys(HashSet<LanguageKeyHash>)
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
		let source = if vfs.file_type(path)?.is_dir() {
			McChangesSource::Directory(path.to_path_buf())
		} else {
			let file = vfs.open(path)?;
			let archive_data = read_to_end(file.file_read, file.file_size_hint).await?;

			McChangesSource::Archive(ZipArchive::parse(archive_data)?)
		};

		let mut mcchanges = Self {
			source,
			layers: None
		};

		mcchanges.layers = match (mcchanges.read_raw(vfs, VERSION_INDEX_FILE).await, version) {
			(Some(index), version) => Some(Self::select_version(&index, version)?),
			(None, None) => None,
			(None, Some(_)) => return Err(McChangesError::NotVersioned)
		};

		Ok(mcchanges)
	}

	/// Parses the version index and returns the layers of the selected version.
	fn select_version(
		index: &[u8],
		version: Option<&str>
	) -> Result<Vec<VersionLayer>, McChangesError> {
		let index = serde_json::from_slice::<VersionIndexFile>(index)
			.map_err(|err| McChangesError::InvalidVersionIndex(err.to_string()))?;

		if index.format != 1 {
			return Err(McChangesError::InvalidVersionIndex(format!(
				"unsupported format {}",
				index.format
			)));
		}

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

		let mut removed = index.removed;
		let mut hashed = index.hashed;

		Ok(index.versions[..=selected_position]
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
			.collect())
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
			return self
				.read_raw(vfs, relative_path)
				.await
				.map(McChangesEntry::Data);
		};

		for layer in layers {
			if let Some(data) = self
				.read_raw(vfs, &format!("{}/{relative_path}", layer.version))
				.await
			{
				return Some(McChangesEntry::Data(data));
			}

			if let Some(hash) = layer.hashed_files.get(relative_path) {
				if hash.starts_with("langkeys:") {
					// A language file whose keys can't be loaded is left as an unmatchable hash
					let keys = self
						.read_raw(
							vfs,
							&format!("{}/{relative_path}{LANGUAGE_KEYS_SUFFIX}", layer.version)
						)
						.await
						.and_then(|keys| parse_language_keys(&keys, hash));

					return Some(
						keys.map_or_else(|| McChangesEntry::Hash(hash.clone()), McChangesEntry::LanguageKeys)
					);
				}

				return Some(McChangesEntry::Hash(hash.clone()));
			}

			if layer.removed_files.contains(relative_path) {
				return None;
			}
		}

		None
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
		if matches!(
			relative_path,
			"pack.mcmeta" | "pack.png" | "manifest.json" | "pack_icon.png" | "bug_pack_icon.png"
		) {
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
			return false;
		};

		match mcchanges_entry {
			McChangesEntry::Hash(hash) => hash_matches(&pack_data, &hash),
			McChangesEntry::Data(mcchanges_data) if relative_path.ends_with(".json") => {
				json_is_identical(&pack_data, &mcchanges_data)
			}
			McChangesEntry::Data(mcchanges_data) => pack_data == mcchanges_data,
			// Identical language files are left out by the language key removal instead
			McChangesEntry::LanguageKeys(_) => false
		}
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

		let texture_is_identical = match mcchanges_texture {
			McChangesEntry::Data(data) => png_is_identical(&pack_texture, &data),
			McChangesEntry::Hash(hash) => hash_matches(&pack_texture, &hash),
			McChangesEntry::LanguageKeys(_) => false
		};

		if !texture_is_identical {
			return false;
		}

		let mut metadata_file_path = OsString::from(texture_file_path);
		metadata_file_path.push(".mcmeta");

		match (
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
		}
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
					keys.contains(&language_key_hash(key, canonical_json(value).as_bytes()))
				})
			}
			(McChangesEntry::LanguageKeys(keys), false) => {
				reduce_legacy_language_file(&pack_data, |key, value| {
					keys.contains(&language_key_hash(key, value.as_bytes()))
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
	let Some((width, height)) = size.split_once('x').and_then(|(width, height)| {
		Some((width.parse::<u32>().ok()?, height.parse::<u32>().ok()?))
	}) else {
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

/// The hash of an entry of a language file, as stored in `.langkeys` files: the first bytes of
/// the SHA-256 digest of the key, a zero byte, and the value. The value is the canonical JSON
/// of the value for JSON language files, and the text after the equals sign for legacy ones.
fn language_key_hash(key: &str, value: &[u8]) -> LanguageKeyHash {
	let mut hasher = Sha256::new();
	hasher.update(key.as_bytes());
	hasher.update([0]);
	hasher.update(value);

	let mut hash = [0; LANGUAGE_KEY_HASH_LENGTH];
	let digest = hasher.finalize();
	let digest: &[u8] = digest.as_ref();
	hash.copy_from_slice(&digest[..LANGUAGE_KEY_HASH_LENGTH]);
	hash
}

/// Parses the contents of a `.langkeys` file: the hashes of the entries of a language file, one
/// after the other. The file must match the `langkeys:<SHA-256 digest>` token that refers to it.
fn parse_language_keys(data: &[u8], token: &str) -> Option<HashSet<LanguageKeyHash>> {
	let expected_digest = token.strip_prefix("langkeys:")?;
	let (hashes, rest) = data.as_chunks::<LANGUAGE_KEY_HASH_LENGTH>();

	if !rest.is_empty() || !hexadecimal(Sha256::digest(data)).eq_ignore_ascii_case(expected_digest) {
		return None;
	}

	Some(hashes.iter().copied().collect())
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
		serde_json::to_vec(&pack_entries)
			.map_or(LanguageFileReduction::Unchanged, |data| {
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

	if size_a != size_b || u64::from(size_a.0) * u64::from(size_a.1) > MAX_PIXEL_COMPARISON_PIXELS
	{
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
