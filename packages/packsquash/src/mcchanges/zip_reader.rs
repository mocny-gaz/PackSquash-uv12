//! A minimal, read-only ZIP archive reader, just capable enough to read the files of
//! vanilla client JARs and resource pack ZIPs that are used as mcchanges packs.
//!
//! Only stored and deflated entries are supported, and encrypted entries are not. Archives with
//! more than 65535 entries (ZIP64) are supported.

use std::collections::HashMap;
use std::io::Read;

use flate2::read::DeflateDecoder;
use thiserror::Error;

const END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x0605_4B50;
const CENTRAL_DIRECTORY_HEADER_SIGNATURE: u32 = 0x0201_4B50;
const LOCAL_FILE_HEADER_SIGNATURE: u32 = 0x0403_4B50;
const ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE: u32 = 0x0606_4B50;
const ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR_SIGNATURE: u32 = 0x0706_4B50;
const ZIP64_EXTRA_FIELD_ID: u16 = 0x0001;

const END_OF_CENTRAL_DIRECTORY_MIN_SIZE: usize = 22;
const CENTRAL_DIRECTORY_HEADER_MIN_SIZE: usize = 46;
const LOCAL_FILE_HEADER_MIN_SIZE: usize = 30;
const ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR_SIZE: usize = 20;

/// The maximum size of an uncompressed entry that will be read. This protects against
/// decompression bombs. No file of a sane mcchanges pack comes close to this limit.
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
	/// The ZIP64 structures of the archive are missing or malformed.
	#[error("the ZIP64 data of the archive is missing or malformed")]
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

fn read_u64(data: &[u8], offset: usize) -> Option<u64> {
	Some(u64::from_le_bytes(
		data.get(offset..offset + 8)?.try_into().ok()?
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

		let mut entry_count = u64::from(read_u16(&data, end_record_offset + 10).ok_or_else(malformed)?);
		let central_directory_size =
			u64::from(read_u32(&data, end_record_offset + 12).ok_or_else(malformed)?);
		let mut central_directory_offset =
			u64::from(read_u32(&data, end_record_offset + 16).ok_or_else(malformed)?);

		// Maxed out fields mean that the real values are in the ZIP64 end of central directory
		// record, which is found with a locator that goes right before the end record
		if entry_count == u64::from(u16::MAX)
			|| central_directory_size == u64::from(u32::MAX)
			|| central_directory_offset == u64::from(u32::MAX)
		{
			let zip64_record_offset = end_record_offset
				.checked_sub(ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR_SIZE)
				.filter(|&locator_offset| {
					read_u32(&data, locator_offset)
						== Some(ZIP64_END_OF_CENTRAL_DIRECTORY_LOCATOR_SIGNATURE)
				})
				.and_then(|locator_offset| read_u64(&data, locator_offset + 8))
				.and_then(|offset| usize::try_from(offset).ok());

			// An archive with exactly 65535 entries has no ZIP64 data, as the values fit
			if let Some(zip64_record_offset) = zip64_record_offset {
				if read_u32(&data, zip64_record_offset)
					!= Some(ZIP64_END_OF_CENTRAL_DIRECTORY_SIGNATURE)
				{
					return Err(ZipReadError::Zip64Unsupported);
				}

				let zip64 = |offset: usize| read_u64(&data, zip64_record_offset + offset);
				(entry_count, central_directory_offset) = (
					zip64(32).ok_or(ZipReadError::Zip64Unsupported)?,
					zip64(48).ok_or(ZipReadError::Zip64Unsupported)?
				);
			}
		}

		let mut entries = HashMap::with_capacity(usize::try_from(entry_count).unwrap_or(0).min(1 << 20));
		let mut offset = usize::try_from(central_directory_offset).map_err(|_| malformed())?;

		for _ in 0..usize::try_from(entry_count).map_err(|_| malformed())? {
			if read_u32(&data, offset).ok_or_else(malformed)? != CENTRAL_DIRECTORY_HEADER_SIGNATURE
				|| data.len() < offset + CENTRAL_DIRECTORY_HEADER_MIN_SIZE
			{
				return Err(ZipReadError::MalformedCentralDirectory);
			}

			let flags = read_u16(&data, offset + 8).ok_or_else(malformed)?;
			let compression_method = read_u16(&data, offset + 10).ok_or_else(malformed)?;
			let crc32 = read_u32(&data, offset + 16).ok_or_else(malformed)?;
			let mut compressed_size = u64::from(read_u32(&data, offset + 20).ok_or_else(malformed)?);
			let mut uncompressed_size = u64::from(read_u32(&data, offset + 24).ok_or_else(malformed)?);
			let name_length = read_u16(&data, offset + 28).ok_or_else(malformed)? as usize;
			let extra_length = read_u16(&data, offset + 30).ok_or_else(malformed)? as usize;
			let comment_length = read_u16(&data, offset + 32).ok_or_else(malformed)? as usize;
			let mut local_header_offset = u64::from(read_u32(&data, offset + 42).ok_or_else(malformed)?);

			let name_start = offset + CENTRAL_DIRECTORY_HEADER_MIN_SIZE;
			let name_bytes = data
				.get(name_start..name_start + name_length)
				.ok_or_else(malformed)?;
			let name = String::from_utf8_lossy(name_bytes).into_owned();

			// Fields that do not fit in 32 bits have their real value in the ZIP64 extra
			// field. It has the values of the maxed out fields only, in this order
			if [uncompressed_size, compressed_size, local_header_offset]
				.contains(&u64::from(u32::MAX))
			{
				let extra = data
					.get(name_start + name_length..name_start + name_length + extra_length)
					.ok_or_else(malformed)?;

				let mut zip64_values = Self::find_extra_field(extra, ZIP64_EXTRA_FIELD_ID)
					.ok_or(ZipReadError::Zip64Unsupported)?
					.as_chunks::<8>()
					.0
					.iter()
					.map(|value| u64::from_le_bytes(*value));

				for field in [
					&mut uncompressed_size,
					&mut compressed_size,
					&mut local_header_offset
				] {
					if *field == u64::from(u32::MAX) {
						*field = zip64_values.next().ok_or(ZipReadError::Zip64Unsupported)?;
					}
				}
			}

			let (Ok(compressed_size), Ok(uncompressed_size), Ok(local_header_offset)) = (
				usize::try_from(compressed_size),
				usize::try_from(uncompressed_size),
				usize::try_from(local_header_offset)
			) else {
				return Err(ZipReadError::MalformedCentralDirectory);
			};

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

	/// Finds the data of an extra field with the given ID in a list of extra fields.
	fn find_extra_field(mut extra: &[u8], id: u16) -> Option<&[u8]> {
		while extra.len() >= 4 {
			let field_id = read_u16(extra, 0)?;
			let field_length = usize::from(read_u16(extra, 2)?);
			let field_data = extra.get(4..4 + field_length)?;

			if field_id == id {
				return Some(field_data);
			}

			extra = &extra[4 + field_length..];
		}

		None
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
