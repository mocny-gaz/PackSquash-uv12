//! Support for baseline packs: packs, usually the vanilla game assets, that the pack being
//! processed is layered on top of. Pack files that do not change anything relative to the
//! baseline are redundant, so they can be left out of the result ZIP file to make it smaller.
//!
//! A baseline pack can also hold the assets of many game versions in a compact way. Such a
//! versioned baseline pack has a `baseline.json` file at its root with this structure:
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

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use spng::{ContextFlags, DecodeFlags, Format};
use thiserror::Error;

use crate::vfs::{VfsPackFileIterEntry, VirtualFileSystem, read_file, read_to_end};

pub(crate) use zip_reader::ZipArchive;
use zip_reader::ZipReadError;

mod zip_reader;

#[cfg(test)]
pub(crate) mod tests;

/// The name of the file that marks a baseline pack as versioned.
const VERSION_INDEX_FILE: &str = "baseline.json";

/// The PNG file signature, followed by the length and type of the IHDR chunk, which
/// always is the first chunk of a PNG file.
const PNG_HEADER: [u8; 16] = [
	0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, b'I', b'H', b'D', b'R'
];

/// Images with more pixels than this are only compared byte by byte, to bound the
/// memory needed to decode them.
const MAX_PIXEL_COMPARISON_PIXELS: u64 = 4096 * 4096;

/// An error that may occur while loading a baseline pack.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum BaselinePackError {
	/// Thrown when an I/O error occurs while reading the baseline pack.
	#[error("I/O error: {0}")]
	IoError(#[from] io::Error),
	/// Thrown when the baseline pack is a file, but not a valid ZIP archive.
	#[error("the baseline pack is neither a directory nor a valid ZIP file: {0}")]
	InvalidArchive(#[from] ZipReadError),
	/// Thrown when the version index of a versioned baseline pack is not valid.
	#[error("the baseline pack version index is not valid: {0}")]
	InvalidVersionIndex(String),
	/// Thrown when the baseline pack holds several game versions, but none was selected.
	#[error("the baseline pack holds several game versions, so baseline_pack_version must be set to one of: {0}")]
	VersionRequired(String),
	/// Thrown when the selected game version is not in the baseline pack.
	#[error("the baseline pack does not hold the game version {version}. It holds: {available}")]
	UnknownVersion {
		/// The version that was selected.
		version: String,
		/// The comma-separated versions that the baseline pack holds.
		available: String
	},
	/// Thrown when a game version was selected, but the baseline pack is not versioned.
	#[error("baseline_pack_version is set, but the baseline pack does not hold several game versions")]
	NotVersioned
}

enum BaselineSource {
	/// A directory in the virtual filesystem that holds the baseline pack files.
	Directory(PathBuf),
	/// A ZIP archive (e.g. a vanilla client JAR) that was loaded in memory.
	Archive(ZipArchive)
}

/// The contents of the `baseline.json` file.
#[derive(Deserialize)]
struct VersionIndexFile {
	format: u32,
	versions: Vec<String>,
	#[serde(default)]
	removed: HashMap<String, Vec<String>>
}

/// A game version of a versioned baseline pack, and the files that it deleted
/// relative to the previous version.
struct VersionLayer {
	version: String,
	removed_files: HashSet<String>
}

/// A loaded baseline pack, ready to be used to find redundant pack files.
pub(crate) struct BaselinePack {
	source: BaselineSource,
	/// For versioned baseline packs, the layers of the selected version and all the
	/// previous ones, from the newest to the oldest.
	layers: Option<Vec<VersionLayer>>
}

/// The result of removing the language keys that a baseline pack already provides from
/// a language file.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum LanguageFileReduction {
	/// There is nothing to remove, or the file can't be safely reduced.
	Unchanged,
	/// Every key is already provided by the baseline pack, so the file is not needed.
	Redundant,
	/// Some keys were removed. The reduced file contents are provided.
	Reduced { data: Vec<u8> }
}

impl BaselinePack {
	/// Loads the baseline pack at the given path, which can be either a directory or a
	/// ZIP archive such as a vanilla client JAR. If the baseline pack holds the assets of
	/// several game versions, `version` selects which one is used.
	pub(crate) async fn open<V: VirtualFileSystem>(
		vfs: &V,
		path: &Path,
		version: Option<&str>
	) -> Result<Self, BaselinePackError> {
		let source = if vfs.file_type(path)?.is_dir() {
			BaselineSource::Directory(path.to_path_buf())
		} else {
			let file = vfs.open(path)?;
			let archive_data = read_to_end(file.file_read, file.file_size_hint).await?;

			BaselineSource::Archive(ZipArchive::parse(archive_data)?)
		};

		let mut baseline_pack = Self {
			source,
			layers: None
		};

		baseline_pack.layers = match (baseline_pack.read_raw(vfs, VERSION_INDEX_FILE).await, version) {
			(Some(index), version) => Some(Self::select_version(&index, version)?),
			(None, None) => None,
			(None, Some(_)) => return Err(BaselinePackError::NotVersioned)
		};

		Ok(baseline_pack)
	}

	/// Parses the version index and returns the layers of the selected version.
	fn select_version(
		index: &[u8],
		version: Option<&str>
	) -> Result<Vec<VersionLayer>, BaselinePackError> {
		let index = serde_json::from_slice::<VersionIndexFile>(index)
			.map_err(|err| BaselinePackError::InvalidVersionIndex(err.to_string()))?;

		if index.format != 1 {
			return Err(BaselinePackError::InvalidVersionIndex(format!(
				"unsupported format {}",
				index.format
			)));
		}

		let available = index.versions.join(", ");
		let Some(version) = version else {
			return Err(BaselinePackError::VersionRequired(available));
		};
		let Some(selected_position) = index.versions.iter().position(|v| v == version) else {
			return Err(BaselinePackError::UnknownVersion {
				version: version.to_owned(),
				available
			});
		};

		let mut removed = index.removed;

		Ok(index.versions[..=selected_position]
			.iter()
			.rev()
			.map(|version| VersionLayer {
				version: version.clone(),
				removed_files: removed
					.remove(version)
					.map(|files| files.into_iter().collect())
					.unwrap_or_default()
			})
			.collect())
	}

	/// Reads a file at the given path, relative to the root of the baseline pack, if it
	/// exists and can be read.
	async fn read_raw<V: VirtualFileSystem>(&self, vfs: &V, path: &str) -> Option<Vec<u8>> {
		match &self.source {
			BaselineSource::Directory(directory) => read_file(vfs, &directory.join(path)).await,
			BaselineSource::Archive(archive) => archive.read(path)
		}
	}

	/// Reads the contents of the baseline pack file with the given relative path, if it
	/// exists and can be read. For versioned baseline packs, this reads the file as it
	/// is in the selected game version.
	async fn read<V: VirtualFileSystem>(&self, vfs: &V, relative_path: &str) -> Option<Vec<u8>> {
		let Some(layers) = &self.layers else {
			return self.read_raw(vfs, relative_path).await;
		};

		for layer in layers {
			if let Some(data) = self
				.read_raw(vfs, &format!("{}/{relative_path}", layer.version))
				.await
			{
				return Some(data);
			}

			if layer.removed_files.contains(relative_path) {
				return None;
			}
		}

		None
	}

	/// Checks whether including the given pack file in the result ZIP file is redundant
	/// because the baseline pack already provides the same data under the same path.
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

		// The pack metadata and icon identify the pack itself, and must always be kept
		if matches!(relative_path, "pack.mcmeta" | "pack.png") {
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

		let (Some(pack_data), Some(baseline_data)) = (
			read_file(vfs, &pack_file.file_path).await,
			self.read(vfs, relative_path).await
		) else {
			return false;
		};

		if relative_path.ends_with(".json") {
			json_is_identical(&pack_data, &baseline_data)
		} else {
			pack_data == baseline_data
		}
	}

	/// Checks whether a texture, along with its animation metadata, is the same as the
	/// baseline one.
	async fn is_texture_redundant<V: VirtualFileSystem>(
		&self,
		vfs: &V,
		texture_relative_path: &str,
		texture_file_path: &Path
	) -> bool {
		let (Some(pack_texture), Some(baseline_texture)) = (
			read_file(vfs, texture_file_path).await,
			self.read(vfs, texture_relative_path).await
		) else {
			return false;
		};

		if !png_is_identical(&pack_texture, &baseline_texture) {
			return false;
		}

		let mut metadata_file_path = OsString::from(texture_file_path);
		metadata_file_path.push(".mcmeta");

		match (
			read_file(vfs, Path::new(&metadata_file_path)).await,
			self.read(vfs, &format!("{texture_relative_path}.mcmeta"))
				.await
		) {
			(None, None) => true,
			(Some(pack_metadata), Some(baseline_metadata)) => {
				json_is_identical(&pack_metadata, &baseline_metadata)
			}
			_ => false
		}
	}

	/// Removes the keys of a language file that have the same value in the language file
	/// of the baseline pack with the same path. As the game falls back to the baseline
	/// pack for the missing keys, the translations the player sees are the same.
	///
	/// Files that are not language files, that do not have a counterpart in the baseline pack,
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

		let reduce: fn(&[u8], &[u8]) -> LanguageFileReduction = if !is_in_language_directory {
			return LanguageFileReduction::Unchanged;
		} else if relative_path.ends_with(".json") {
			reduce_json_language_file
		} else if relative_path.ends_with(".lang") {
			reduce_legacy_language_file
		} else {
			return LanguageFileReduction::Unchanged;
		};

		let (Some(pack_data), Some(baseline_data)) = (
			read_file(vfs, &pack_file.file_path).await,
			self.read(vfs, relative_path).await
		) else {
			return LanguageFileReduction::Unchanged;
		};

		reduce(&pack_data, &baseline_data)
	}
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

	let parse = |data: &[u8]| serde_json::from_slice::<serde_json::Value>(strip_bom(data)).ok();

	matches!((parse(a), parse(b)), (Some(a), Some(b)) if a == b)
}

/// Removes the entries of a JSON language file that the baseline language file has with the
/// same value.
fn reduce_json_language_file(pack: &[u8], baseline: &[u8]) -> LanguageFileReduction {
	let parse = |data: &[u8]| match serde_json::from_slice(strip_bom(data)) {
		Ok(serde_json::Value::Object(entries)) => Some(entries),
		_ => None
	};

	let (Some(mut pack_entries), Some(baseline_entries)) = (parse(pack), parse(baseline)) else {
		return LanguageFileReduction::Unchanged;
	};

	let entry_count = pack_entries.len();
	pack_entries.retain(|key, value| baseline_entries.get(key) != Some(&*value));

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

/// Removes the entries of a legacy language file (`key=value` lines) that the baseline
/// language file has with the same value. Comments, blank lines and line endings are kept.
fn reduce_legacy_language_file(pack: &[u8], baseline: &[u8]) -> LanguageFileReduction {
	let (Ok(pack), Ok(baseline)) = (std::str::from_utf8(pack), std::str::from_utf8(baseline))
	else {
		return LanguageFileReduction::Unchanged;
	};

	// Like the game does, ignore blank lines, comments, and lines without an equals sign,
	// and do not trim anything but the line terminator
	fn entry(line: &str) -> Option<(&str, &str)> {
		let line = line.trim_end_matches(['\r', '\n']);

		if line.is_empty() || line.starts_with('#') {
			None
		} else {
			line.split_once('=')
		}
	}

	// When a key appears several times, the game uses the last value, so that is the one
	// to compare with. A key is only removed if the value the game would use is provided by
	// the baseline, which means removing every line of that key
	let baseline_values: HashMap<_, _> = baseline.split_inclusive('\n').filter_map(entry).collect();
	let pack_values: HashMap<_, _> = pack.split_inclusive('\n').filter_map(entry).collect();

	let mut reduced = String::with_capacity(pack.len());
	let (mut removed_entries, mut kept_entries) = (0_usize, 0_usize);

	for line in pack.split_inclusive('\n') {
		match entry(line) {
			Some((key, _)) if baseline_values.get(key) == pack_values.get(key) => {
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

	for pixel in pixels.chunks_exact_mut(4) {
		if pixel[3] == 0 {
			pixel[..3].fill(0);
		}
	}

	Some(pixels)
}
