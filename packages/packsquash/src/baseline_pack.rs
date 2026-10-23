//! Support for baseline packs: packs, usually the vanilla game assets, that the pack being
//! processed is layered on top of. Pack files that do not change anything relative to the
//! baseline are redundant, so they can be left out of the result ZIP file to make it smaller.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use spng::{ContextFlags, DecodeFlags, Format};
use thiserror::Error;

use crate::vfs::{VfsPackFileIterEntry, VirtualFileSystem, read_file, read_to_end};

pub(crate) use zip_reader::ZipArchive;
use zip_reader::ZipReadError;

mod zip_reader;

#[cfg(test)]
pub(crate) mod tests;

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
	InvalidArchive(#[from] ZipReadError)
}

enum BaselineSource {
	/// A directory in the virtual filesystem that holds the baseline pack files.
	Directory(PathBuf),
	/// A ZIP archive (e.g. a vanilla client JAR) that was loaded in memory.
	Archive(ZipArchive)
}

/// A loaded baseline pack, ready to be used to find redundant pack files.
pub(crate) struct BaselinePack {
	source: BaselineSource
}

impl BaselinePack {
	/// Loads the baseline pack at the given path, which can be either a directory or a
	/// ZIP archive such as a vanilla client JAR.
	pub(crate) async fn open<V: VirtualFileSystem>(
		vfs: &V,
		path: &Path
	) -> Result<Self, BaselinePackError> {
		if vfs.file_type(path)?.is_dir() {
			return Ok(Self {
				source: BaselineSource::Directory(path.to_path_buf())
			});
		}

		let file = vfs.open(path)?;
		let archive_data = read_to_end(file.file_read, file.file_size_hint).await?;

		Ok(Self {
			source: BaselineSource::Archive(ZipArchive::parse(archive_data)?)
		})
	}

	/// Reads the contents of the baseline pack file with the given relative path, if it
	/// exists and can be read.
	async fn read<V: VirtualFileSystem>(&self, vfs: &V, relative_path: &str) -> Option<Vec<u8>> {
		match &self.source {
			BaselineSource::Directory(directory) => {
				read_file(vfs, &directory.join(relative_path)).await
			}
			BaselineSource::Archive(archive) => archive.read(relative_path)
		}
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
}

/// Checks whether two JSON documents have the same contents, regardless of how they are
/// formatted or the order of their object keys. If any of them is not valid JSON, they
/// are compared byte by byte.
fn json_is_identical(a: &[u8], b: &[u8]) -> bool {
	if a == b {
		return true;
	}

	let parse = |data: &[u8]| {
		serde_json::from_slice::<serde_json::Value>(
			data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data)
		)
		.ok()
	};

	matches!((parse(a), parse(b)), (Some(a), Some(b)) if a == b)
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

	for pixel in pixels.chunks_exact_mut(4) {
		if pixel[3] == 0 {
			pixel[..3].fill(0);
		}
	}

	Some(pixels)
}
