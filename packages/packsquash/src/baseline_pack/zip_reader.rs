//! A minimal, read-only ZIP archive reader, just capable enough to read the files of
//! vanilla client JARs and resource pack ZIPs that are used as baseline packs.
//!
//! Only stored and deflated entries are supported. ZIP64 archives and encrypted entries
//! are not, which is fine for the archives this module is meant to read.

use std::collections::HashMap;
use std::io::Read;

use flate2::read::DeflateDecoder;
use thiserror::Error;

const END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x0605_4B50;
const CENTRAL_DIRECTORY_HEADER_SIGNATURE: u32 = 0x0201_4B50;
const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x0403_4B50;

const END_OF_CENTRAL_DIRECTORY_MIN_SIZE: usize = 22;
const CENTRAL_DIRECTORY_HEADER_MIN_SIZE: usize = 46;
const LOCAL_FILE_HEADER_MIN_SIZE: usize = 30;

/// The maximum size of an uncompressed entry that will be read. This protects against
/// decompression bombs. No file of a sane baseline pack comes close to this limit.
const MAX_ENTRY_SIZE: usize = 256 * 1024 * 1024;

/// An error that may occur while parsing a ZIP archive.
#[derive(Error, Debug)]
pub enum ZipReadError {
	/// The end of central directory record could not be found, so the data is
	/// most likely not a ZIP archive.
	#[error("the end of central directory record was not found")]
	MissingEndOfCentralDirectory,
	/// The central directory is truncated or has invalid contents.
	#[error("the central directory is truncated or malformed")]
	MalformedCentralDirectory,
	/// The archive uses the ZIP64 extensions, which are not supported.
	#[error("ZIP64 archives are not supported")]
	Zip64Unsupported
}

#[derive(Debug)]
struct Entry {
	local_header_offset: usize,
	compressed_size: usize,
	uncompressed_size: usize,
	crc32: u32,
	compression_method: u16
}

/// An in-memory ZIP archive whose entries can be read by name.
#[derive(Debug)]
pub struct ZipArchive {
	data: Vec<u8>,
	entries: HashMap<String, Entry>
}

fn read_u16(data: &[u8], offset: usize) -> Option<u16> {
	Some(u16::from_le_bytes(
		data.get(offset..offset + 2)?.try_into().ok()?
	))
}

fn read_u32(data: &[u8], offset: usize) -> Option<u32> {
	Some(u32::from_le_bytes(
		data.get(offset..offset + 4)?.try_into().ok()?
	))
}

impl ZipArchive {
	/// Parses the central directory of the provided ZIP archive data, taking
	/// ownership of it so entries can be lazily read later.
	pub fn parse(data: Vec<u8>) -> Result<Self, ZipReadError> {
		let end_record_offset = Self::find_end_of_central_directory(&data)
			.ok_or(ZipReadError::MissingEndOfCentralDirectory)?;

		let malformed = || ZipReadError::MalformedCentralDirectory;

		let entry_count = read_u16(&data, end_record_offset + 10).ok_or_else(malformed)?;
		let central_directory_size = read_u32(&data, end_record_offset + 12).ok_or_else(malformed)?;
		let central_directory_offset =
			read_u32(&data, end_record_offset + 16).ok_or_else(malformed)?;

		if entry_count == u16::MAX
			|| central_directory_size == u32::MAX
			|| central_directory_offset == u32::MAX
		{
			return Err(ZipReadError::Zip64Unsupported);
		}

		let mut entries = HashMap::with_capacity(entry_count.into());
		let mut offset = central_directory_offset as usize;

		for _ in 0..entry_count {
			if read_u32(&data, offset).ok_or_else(malformed)? != CENTRAL_DIRECTORY_HEADER_SIGNATURE
				|| data.len() < offset + CENTRAL_DIRECTORY_HEADER_MIN_SIZE
			{
				return Err(ZipReadError::MalformedCentralDirectory);
			}

			let flags = read_u16(&data, offset + 8).ok_or_else(malformed)?;
			let compression_method = read_u16(&data, offset + 10).ok_or_else(malformed)?;
			let crc32 = read_u32(&data, offset + 16).ok_or_else(malformed)?;
			let compressed_size = read_u32(&data, offset + 20).ok_or_else(malformed)? as usize;
			let uncompressed_size = read_u32(&data, offset + 24).ok_or_else(malformed)? as usize;
			let name_length = read_u16(&data, offset + 28).ok_or_else(malformed)? as usize;
			let extra_length = read_u16(&data, offset + 30).ok_or_else(malformed)? as usize;
			let comment_length = read_u16(&data, offset + 32).ok_or_else(malformed)? as usize;
			let local_header_offset = read_u32(&data, offset + 42).ok_or_else(malformed)? as usize;

			let name_start = offset + CENTRAL_DIRECTORY_HEADER_MIN_SIZE;
			let name_bytes = data
				.get(name_start..name_start + name_length)
				.ok_or_else(malformed)?;
			let name = String::from_utf8_lossy(name_bytes).into_owned();

			// Skip directories and encrypted entries (bit 0 of the flags), which cannot be
			// read anyway. Not having them in the entry map makes them be treated as
			// missing files
			if !name.ends_with('/') && flags & 1 == 0 {
				entries.insert(
					name,
					Entry {
						local_header_offset,
						compressed_size,
						uncompressed_size,
						crc32,
						compression_method
					}
				);
			}

			offset = name_start + name_length + extra_length + comment_length;
		}

		Ok(Self { data, entries })
	}

	/// Looks for the end of central directory record, which is at the end of the archive,
	/// optionally followed by a comment of up to 65535 bytes.
	fn find_end_of_central_directory(data: &[u8]) -> Option<usize> {
		let last_candidate = data.len().checked_sub(END_OF_CENTRAL_DIRECTORY_MIN_SIZE)?;
		let first_candidate = last_candidate.saturating_sub(u16::MAX.into());

		(first_candidate..=last_candidate)
			.rev()
			.find(|&offset| read_u32(data, offset) == Some(END_OF_CENTRAL_DIRECTORY_SIGNATURE))
	}

	/// Returns whether the archive has a file entry with the given name.
	#[cfg(test)]
	pub fn contains(&self, name: &str) -> bool {
		self.entries.contains_key(name)
	}

	/// Reads and decompresses the entry with the given name. `None` is returned if the
	/// entry does not exist, uses an unsupported compression method, or is corrupt.
	pub fn read(&self, name: &str) -> Option<Vec<u8>> {
		let entry = self.entries.get(name)?;

		if entry.uncompressed_size > MAX_ENTRY_SIZE {
			return None;
		}

		let header_offset = entry.local_header_offset;
		if read_u32(&self.data, header_offset)? != LOCAL_FILE_HEADER_SIGNATURE {
			return None;
		}

		// The extra field in the local header may have a different length than the one in
		// the central directory, so it must be read from the local header
		let name_length = read_u16(&self.data, header_offset + 26)? as usize;
		let extra_length = read_u16(&self.data, header_offset + 28)? as usize;
		let data_start = header_offset + LOCAL_FILE_HEADER_MIN_SIZE + name_length + extra_length;
		let compressed = self
			.data
			.get(data_start..data_start + entry.compressed_size)?;

		let decompressed = match entry.compression_method {
			0 => compressed.to_vec(),
			8 => {
				let mut decompressed = Vec::with_capacity(entry.uncompressed_size);

				// Read one byte more than expected to detect entries that decompress to
				// more data than they claim
				DeflateDecoder::new(compressed)
					.take(entry.uncompressed_size as u64 + 1)
					.read_to_end(&mut decompressed)
					.ok()?;

				decompressed
			}
			_ => return None
		};

		(decompressed.len() == entry.uncompressed_size
			&& crc32fast::hash(&decompressed) == entry.crc32)
			.then_some(decompressed)
	}
}
