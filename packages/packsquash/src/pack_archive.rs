//! Support for packs that come inside archives, and for finding packs in a directory tree.
//!
//! An archive is recognized by its first bytes, never by its extension. Supported without any other
//! program: ZIP, 7z, TAR, and TAR compressed with gzip, xz, Zstandard or bzip2. Formats that need
//! another program (RAR) are extracted with `bsdtar`, `tar`, `7z` or `unrar`, if one of them is installed.
//!
//! A pack is found by its contents: a Java Edition pack has a `pack.mcmeta` file and an `assets` or `data`
//! directory next to it, and a Bedrock Edition resource pack has a `manifest.json` file with a `resources`
//! module. The pack may be in a subdirectory of the archive.
//!
//! Extraction is careful: names that would leave the destination directory, links and special files are
//! skipped, and the number of files and the total size are limited, so that a damaged or hostile archive
//! can not fill the disk.

use crate::debug_log::detail;
use std::{
	collections::{BTreeMap, BTreeSet},
	ffi::OsString,
	fs::{self, File},
	io::{self, BufReader, Read},
	path::{Path, PathBuf},
	process::{Command, Stdio}
};
use thiserror::Error;

/// The default limit of the total size of the files extracted from an archive.
pub const DEFAULT_MAX_EXTRACTED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
/// The default limit of the number of entries of an archive.
pub const DEFAULT_MAX_ENTRIES: u64 = 500_000;
/// The default number of directory levels that are searched for packs.
pub const DEFAULT_MAX_PACK_DEPTH: usize = 4;

/// Bigger `manifest.json` files are not read.
const MAX_MANIFEST_SIZE: u64 = 1024 * 1024;

/// How the content of a TAR archive is compressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TarCompression {
	/// No compression.
	None,
	/// Compressed with gzip.
	Gzip,
	/// Compressed with xz.
	Xz,
	/// Compressed with Zstandard.
	Zstd,
	/// Compressed with bzip2.
	Bzip2
}

/// A kind of archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveFormat {
	/// A ZIP archive.
	Zip,
	/// A 7z archive.
	SevenZip,
	/// A TAR archive, maybe compressed.
	Tar(TarCompression),
	/// An archive that another program has to extract, like RAR.
	Helper
}

impl ArchiveFormat {
	/// A short name for messages.
	pub fn name(self) -> &'static str {
		match self {
			Self::Zip => "ZIP",
			Self::SevenZip => "7z",
			Self::Tar(TarCompression::None) => "TAR",
			Self::Tar(TarCompression::Gzip) => "TAR + gzip",
			Self::Tar(TarCompression::Xz) => "TAR + xz",
			Self::Tar(TarCompression::Zstd) => "TAR + Zstandard",
			Self::Tar(TarCompression::Bzip2) => "TAR + bzip2",
			Self::Helper => "RAR or another format that needs a helper program"
		}
	}
}

/// An error while reading or extracting an archive.
#[derive(Error, Debug)]
pub enum ArchiveError {
	#[error("I/O error: {0}")]
	/// Reading or writing a file failed.
	Io(#[from] io::Error),
	#[error("the archive is damaged, protected with a password, or not supported: {0}")]
	/// The archive can not be read.
	Format(String),
	#[error("the archive is too big: {0}")]
	/// The archive is over a limit.
	TooBig(String),
	#[error(
		"this archive needs a helper program, but none was found (tried bsdtar, tar, 7z, 7zz, 7za and unrar). Install 7-Zip, or use a ZIP, 7z or TAR archive"
	)]
	/// No helper program was found.
	NoHelper,
	#[error("no pack was found in the archive")]
	/// The archive has no pack.
	NoPack
}

/// What an archive entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
	/// A file.
	File,
	/// A directory.
	Directory,
	/// A link, a device or anything else that is never extracted.
	Other
}

/// An entry of an archive, as it is stored.
#[derive(Debug, Clone)]
pub struct Entry {
	/// The name of the entry, as it is stored.
	pub name: String,
	/// What the entry is.
	pub kind: EntryKind,
	/// The size of the contents, as the entry says.
	pub size: u64
}

/// What the visitor of [`walk_archive`] wants to do next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visit {
	Continue,
	Stop
}

type Visitor<'a> = dyn FnMut(&Entry, &mut dyn Read) -> Result<Visit, ArchiveError> + 'a;

fn format_error(error: impl std::fmt::Display) -> ArchiveError {
	ArchiveError::Format(error.to_string())
}

/// Reads as many bytes as possible into the buffer, and returns how many were read.
fn read_up_to(reader: &mut impl Read, buffer: &mut [u8]) -> io::Result<usize> {
	let mut filled = 0;

	while filled < buffer.len() {
		match reader.read(&mut buffer[filled..]) {
			Ok(0) => break,
			Ok(read) => filled += read,
			Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
			Err(error) => return Err(error)
		}
	}

	Ok(filled)
}

fn is_tar_header(block: &[u8]) -> bool {
	block.len() >= 262 && &block[257..262] == b"ustar"
}

/// Opens the stream of a TAR archive, decompressing it if needed.
fn open_tar_stream(path: &Path, compression: TarCompression) -> Result<Box<dyn Read>, ArchiveError> {
	let file = BufReader::new(File::open(path)?);

	Ok(match compression {
		TarCompression::None => Box::new(file),
		TarCompression::Gzip => Box::new(flate2::read::MultiGzDecoder::new(file)),
		TarCompression::Xz => Box::new(lzma_rust2::XzReader::new(file, true)),
		TarCompression::Zstd => {
			Box::new(ruzstd::decoding::StreamingDecoder::new(file).map_err(format_error)?)
		}
		TarCompression::Bzip2 => Box::new(bzip2::read::BzDecoder::new(file))
	})
}

/// Finds out what kind of archive a file is, by looking at its first bytes. Returns `None` if the file is not
/// an archive of a kind that is supported (a compressed file that does not hold a TAR archive is not one).
pub fn detect_format(path: &Path) -> io::Result<Option<ArchiveFormat>> {
	if !fs::metadata(path)?.is_file() {
		return Ok(None);
	}

	let mut head = [0; 512];
	let read = read_up_to(&mut File::open(path)?, &mut head)?;
	let head = &head[..read];

	detail!(
		"Archive {}: the first bytes of the file are {}",
		path.display(),
		head.iter()
			.take(16)
			.map(|byte| format!("{byte:02x}"))
			.collect::<Vec<_>>()
			.join(" ")
	);

	let compression = if head.starts_with(&[0x1F, 0x8B]) {
		Some(TarCompression::Gzip)
	} else if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0]) {
		Some(TarCompression::Xz)
	} else if head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD]) {
		Some(TarCompression::Zstd)
	} else if head.starts_with(b"BZh") {
		Some(TarCompression::Bzip2)
	} else {
		None
	};

	Ok(
		if head.starts_with(b"PK\x03\x04") || head.starts_with(b"PK\x05\x06") {
			Some(ArchiveFormat::Zip)
		} else if head.starts_with(&[0x37, 0x7A, 0xBC, 0xAF, 0x27, 0x1C]) {
			Some(ArchiveFormat::SevenZip)
		} else if head.starts_with(b"Rar!\x1A\x07") {
			Some(ArchiveFormat::Helper)
		} else if let Some(compression) = compression {
			// A compressed file is an archive only if it holds a TAR archive
			let mut block = [0; 512];
			let tar = open_tar_stream(path, compression)
				.ok()
				.and_then(|mut stream| read_up_to(&mut stream, &mut block).ok())
				.is_some_and(|read| is_tar_header(&block[..read]));

			tar.then_some(ArchiveFormat::Tar(compression))
		} else if is_tar_header(head) {
			Some(ArchiveFormat::Tar(TarCompression::None))
		} else {
			None
		}
	)
}

/// Goes through the entries of an archive, in the order in which they are stored. The visitor gets the entry
/// and a reader of its contents, which it may leave unread.
fn walk_archive(
	path: &Path,
	format: ArchiveFormat,
	visitor: &mut Visitor<'_>
) -> Result<(), ArchiveError> {
	match format {
		ArchiveFormat::Zip => {
			let mut archive =
				zip::ZipArchive::new(BufReader::new(File::open(path)?)).map_err(format_error)?;

			for index in 0..archive.len() {
				let mut file = archive.by_index(index).map_err(format_error)?;
				let kind = if file.is_symlink() {
					EntryKind::Other
				} else if file.is_dir() {
					EntryKind::Directory
				} else if file.is_file() {
					EntryKind::File
				} else {
					EntryKind::Other
				};
				let entry = Entry {
					name: file.name().map_err(format_error)?.into_owned(),
					kind,
					size: file.size()
				};

				if visitor(&entry, &mut file)? == Visit::Stop {
					break;
				}
			}

			Ok(())
		}
		ArchiveFormat::SevenZip => {
			let mut archive = sevenz_rust2::ArchiveReader::new(
				BufReader::new(File::open(path)?),
				sevenz_rust2::Password::empty()
			)
			.map_err(format_error)?;

			// The 7z library only allows its own error type in the callback, so ours waits here
			let mut failure = None;

			archive
				.for_each_entries(|entry, reader| {
					let item = Entry {
						name: entry.name().to_string(),
						kind: if entry.is_directory() {
							EntryKind::Directory
						} else {
							EntryKind::File
						},
						size: entry.size()
					};

					match visitor(&item, reader) {
						Ok(Visit::Continue) => Ok(true),
						Ok(Visit::Stop) => Ok(false),
						Err(error) => {
							failure = Some(error);
							Ok(false)
						}
					}
				})
				.map_err(format_error)?;

			failure.map_or(Ok(()), Err)
		}
		ArchiveFormat::Tar(compression) => {
			let mut archive = tar::Archive::new(open_tar_stream(path, compression)?);

			for entry in archive.entries()? {
				let mut entry = entry?;
				let entry_type = entry.header().entry_type();
				let item = Entry {
					name: entry.path()?.to_string_lossy().into_owned(),
					kind: if entry_type.is_file() {
						EntryKind::File
					} else if entry_type.is_dir() {
						EntryKind::Directory
					} else {
						EntryKind::Other
					},
					size: entry.size()
				};

				if visitor(&item, &mut entry)? == Visit::Stop {
					break;
				}
			}

			Ok(())
		}
		ArchiveFormat::Helper => Err(ArchiveError::NoHelper)
	}
}

/// Cleans up the name of an archive entry: separators become `/`, empty and `.` parts disappear. Returns `None`
/// if the name is not safe to extract: it is absolute, goes up with `..`, has a drive or stream marker (`:`),
/// or has a NUL character.
pub fn safe_relative_name(name: &str) -> Option<String> {
	let name = name.replace('\\', "/");

	if name.starts_with('/') {
		return None;
	}

	let mut parts = Vec::new();

	for part in name.split('/') {
		match part {
			"" | "." => {}
			".." => return None,
			part if part.contains(['\0', ':']) => return None,
			part => parts.push(part)
		}
	}

	(!parts.is_empty()).then(|| parts.join("/"))
}

/// What [`extract_archive`] does not do, and what it did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExtractSummary {
	/// How many files were extracted.
	pub files: u64,
	/// How many bytes were extracted.
	pub bytes: u64,
	/// Entries with a name that would leave the destination directory.
	pub skipped_unsafe: u64,
	/// Links, devices and other entries that are not files or directories.
	pub skipped_special: u64
}

/// The limits of an extraction.
#[derive(Debug, Clone, Copy)]
pub struct ExtractLimits {
	/// The most bytes that are extracted.
	pub max_bytes: u64,
	/// The most entries that are read.
	pub max_entries: u64
}

impl Default for ExtractLimits {
	fn default() -> Self {
		Self {
			max_bytes: DEFAULT_MAX_EXTRACTED_BYTES,
			max_entries: DEFAULT_MAX_ENTRIES
		}
	}
}

fn is_under(name: &str, prefixes: &[String]) -> bool {
	prefixes.is_empty()
		|| prefixes.iter().any(|prefix| {
			prefix.is_empty()
				|| name == prefix
				|| name
					.strip_prefix(prefix.as_str())
					.is_some_and(|rest| rest.starts_with('/'))
		})
}

/// Extracts the files of an archive into a directory. If `only_under` is not empty, only the files in those
/// directories of the archive (`""` is the root) are extracted. The archive must not be of the
/// [`ArchiveFormat::Helper`] format: use [`extract_with_helper`] for that.
pub fn extract_archive(
	archive: &Path,
	format: ArchiveFormat,
	destination: &Path,
	only_under: &[String],
	limits: ExtractLimits
) -> Result<ExtractSummary, ArchiveError> {
	let mut summary = ExtractSummary::default();
	let mut entries = 0_u64;

	fs::create_dir_all(destination)?;

	walk_archive(archive, format, &mut |entry, reader| {
		entries += 1;
		if entries > limits.max_entries {
			return Err(ArchiveError::TooBig(format!(
				"it has more than {} entries",
				limits.max_entries
			)));
		}

		let Some(name) = safe_relative_name(&entry.name) else {
			detail!(
				"Archive entry {:?} was not extracted, because its name is not safe (it goes up, is absolute, has a drive letter or backslash tricks, or is empty)",
				entry.name
			);
			summary.skipped_unsafe += 1;
			return Ok(Visit::Continue);
		};

		if !is_under(&name, only_under) {
			return Ok(Visit::Continue);
		}

		let target = destination.join(name.split('/').collect::<PathBuf>());

		match entry.kind {
			EntryKind::Directory => fs::create_dir_all(&target)?,
			EntryKind::File => {
				if let Some(parent) = target.parent() {
					fs::create_dir_all(parent)?;
				}

				// Count what is really written: the size in the entry can not be trusted
				let remaining = limits.max_bytes.saturating_sub(summary.bytes);
				let copied = io::copy(
					&mut reader.take(remaining.saturating_add(1)),
					&mut File::create(&target)?
				)?;

				if copied > remaining {
					return Err(ArchiveError::TooBig(format!(
						"it holds more than {} bytes of files",
						limits.max_bytes
					)));
				}

				summary.bytes += copied;
				summary.files += 1;
			}
			EntryKind::Other => {
				detail!(
					"Archive entry {name} was not extracted, because it is a link or another entry that is not a file or a directory"
				);
				summary.skipped_special += 1;
			}
		}

		Ok(Visit::Continue)
	})?;

	detail!(
		"Extracted {} files with {} bytes from the archive {} to {}. Skipped {} entries with an unsafe name and {} links or special entries",
		summary.files,
		summary.bytes,
		archive.display(),
		destination.display(),
		summary.skipped_unsafe,
		summary.skipped_special
	);

	Ok(summary)
}

/// Extracts an archive with a program that is installed: `bsdtar` (and `tar` of Windows 10 and 11, which is
/// bsdtar), 7-Zip or `unrar`. They read many formats that this library does not.
pub fn extract_with_helper(archive: &Path, destination: &Path) -> Result<(), ArchiveError> {
	fs::create_dir_all(destination)?;

	let archive = archive.as_os_str();
	let destination = destination.as_os_str();

	let mut output_option = OsString::from("-o");
	output_option.push(destination);
	let mut unrar_destination = destination.to_os_string();
	unrar_destination.push(std::path::MAIN_SEPARATOR_STR);

	let attempts: [(&str, Vec<&std::ffi::OsStr>); 6] = [
		(
			"bsdtar",
			vec![
				"-x".as_ref(),
				"-f".as_ref(),
				archive,
				"-C".as_ref(),
				destination,
			]
		),
		(
			"tar",
			vec![
				"-x".as_ref(),
				"-f".as_ref(),
				archive,
				"-C".as_ref(),
				destination,
			]
		),
		(
			"7z",
			vec![
				"x".as_ref(),
				"-y".as_ref(),
				output_option.as_os_str(),
				archive,
			]
		),
		(
			"7zz",
			vec![
				"x".as_ref(),
				"-y".as_ref(),
				output_option.as_os_str(),
				archive,
			]
		),
		(
			"7za",
			vec![
				"x".as_ref(),
				"-y".as_ref(),
				output_option.as_os_str(),
				archive,
			]
		),
		(
			"unrar",
			vec![
				"x".as_ref(),
				"-y".as_ref(),
				"-idq".as_ref(),
				archive,
				unrar_destination.as_os_str(),
			]
		)
	];

	for (program, arguments) in attempts {
		let mut command = Command::new(program);
		command.args(&arguments).stdin(Stdio::null());

		let succeeded = if crate::debug_log::enabled() {
			// Keep what the program says, to know why it failed
			let command_line = format!(
				"{program} {}",
				arguments
					.iter()
					.map(|argument| argument.to_string_lossy())
					.collect::<Vec<_>>()
					.join(" ")
			);

			match command.output() {
				Ok(output) => {
					let shorten = |bytes: &[u8]| {
						let text = String::from_utf8_lossy(bytes);

						text.chars()
							.take(4096)
							.collect::<String>()
							.trim()
							.to_string()
					};

					detail!(
						"Helper program run: {command_line}. Exit status: {}. Output: {:?}. Error output: {:?}",
						output.status,
						shorten(&output.stdout),
						shorten(&output.stderr)
					);

					output.status.success()
				}
				Err(error) => {
					detail!(
						"Helper program {program} could not be run ({command_line}): {error} [{:?}]",
						error.kind()
					);

					false
				}
			}
		} else {
			command
				.stdout(Stdio::null())
				.stderr(Stdio::null())
				.status()
				.is_ok_and(|status| status.success())
		};

		if succeeded {
			return Ok(());
		}
	}

	Err(ArchiveError::NoHelper)
}

/// Deletes links and other entries that are not files or directories from what a helper program extracted, and
/// checks the limits. Helper programs do not apply the safety rules of this library, so a link in the
/// archive could otherwise make PackSquash read files outside of the extracted pack.
fn clean_helper_output(directory: &Path, limits: ExtractLimits) -> Result<(), ArchiveError> {
	let mut pending = vec![directory.to_path_buf()];
	let mut bytes = 0_u64;
	let mut entries = 0_u64;

	while let Some(current) = pending.pop() {
		for entry in fs::read_dir(&current)? {
			let entry = entry?;
			let kind = entry.file_type()?;

			entries += 1;
			if entries > limits.max_entries {
				return Err(ArchiveError::TooBig(format!(
					"it has more than {} entries",
					limits.max_entries
				)));
			}

			if kind.is_dir() {
				pending.push(entry.path());
			} else if kind.is_file() {
				bytes = bytes.saturating_add(entry.metadata()?.len());

				if bytes > limits.max_bytes {
					return Err(ArchiveError::TooBig(format!(
						"it holds more than {} bytes of files",
						limits.max_bytes
					)));
				}
			} else {
				// A link, a device or anything else. A link to a directory needs another call on some systems
				let path = entry.path();
				detail!(
					"Removed {}, which a helper program extracted and is a link or another entry that is not a file or a directory",
					path.display()
				);
				fs::remove_file(&path).or_else(|_| fs::remove_dir(&path))?;
			}
		}
	}

	Ok(())
}

fn is_bedrock_resource_manifest(data: &[u8]) -> bool {
	let Ok(serde_json::Value::Object(manifest)) =
		serde_json::from_reader(json_comments::StripComments::new(data))
	else {
		return false;
	};

	manifest
		.get("header")
		.is_some_and(|header| header.is_object())
		&& manifest
			.get("modules")
			.and_then(|modules| modules.as_array())
			.is_some_and(|modules| {
				modules.iter().any(|module| {
					module
						.get("type")
						.and_then(|kind| kind.as_str())
						.is_some_and(|kind| kind.eq_ignore_ascii_case("resources"))
				})
			})
}

/// Directories that are never searched for packs.
fn is_ignored_directory_name(name: &str) -> bool {
	name == "__MACOSX" || name.starts_with('.')
}

/// Checks whether a directory is the root of a pack: a Java Edition pack (`pack.mcmeta` and `assets` or
/// `data`) or a Bedrock Edition resource pack (`manifest.json` with a `resources` module).
pub fn is_pack_directory(directory: &Path) -> bool {
	if directory.join("pack.mcmeta").is_file()
		&& (directory.join("assets").is_dir() || directory.join("data").is_dir())
	{
		return true;
	}

	let manifest = directory.join("manifest.json");

	manifest.is_file()
		&& fs::metadata(&manifest).is_ok_and(|metadata| metadata.len() <= MAX_MANIFEST_SIZE)
		&& fs::read(&manifest).is_ok_and(|data| is_bedrock_resource_manifest(&data))
}

/// Finds the packs in a directory tree, looking at most `max_depth` levels down. The directory itself may be a
/// pack. A pack is never searched for other packs. The result is sorted.
pub fn find_pack_directories(directory: &Path, max_depth: usize) -> Vec<PathBuf> {
	fn search(directory: &Path, depth: usize, max_depth: usize, found: &mut Vec<PathBuf>) {
		if is_pack_directory(directory) {
			found.push(directory.to_path_buf());
			return;
		}

		if depth >= max_depth {
			return;
		}

		let Ok(entries) = fs::read_dir(directory) else {
			return;
		};
		let mut subdirectories: Vec<_> = entries
			.filter_map(Result::ok)
			.filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
			.filter(|entry| !is_ignored_directory_name(&entry.file_name().to_string_lossy()))
			.map(|entry| entry.path())
			.collect();
		subdirectories.sort();

		for subdirectory in subdirectories {
			search(&subdirectory, depth + 1, max_depth, found);
		}
	}

	let mut found = Vec::new();
	search(directory, 0, max_depth, &mut found);
	found
}

/// What is known about an archive after reading its list of entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveScan {
	/// The kind of archive.
	pub format: ArchiveFormat,
	/// The directories of the archive that are the root of a pack (`""` is the root of the archive). `None`
	/// for archives that need a helper program: the packs are only known after extracting them.
	pub pack_roots: Option<Vec<String>>
}

/// Reads the list of entries of an archive, and finds the packs in it, looking at most `max_depth` levels down.
/// Returns `None` if the file is not an archive that is supported.
pub fn scan_archive(path: &Path, max_depth: usize) -> Result<Option<ArchiveScan>, ArchiveError> {
	let Some(format) = detect_format(path)? else {
		return Ok(None);
	};

	if format == ArchiveFormat::Helper {
		detail!(
			"The archive {} needs a helper program to be read, so its packs are only known after it is extracted",
			path.display()
		);

		return Ok(Some(ArchiveScan {
			format,
			pack_roots: None
		}));
	}

	let mut files = BTreeSet::new();
	let mut manifests = BTreeMap::new();
	let mut entries = 0_u64;

	walk_archive(path, format, &mut |entry, reader| {
		entries += 1;
		if entries > DEFAULT_MAX_ENTRIES {
			return Err(ArchiveError::TooBig(format!(
				"it has more than {DEFAULT_MAX_ENTRIES} entries"
			)));
		}

		if entry.kind != EntryKind::File {
			return Ok(Visit::Continue);
		}

		let Some(name) = safe_relative_name(&entry.name) else {
			return Ok(Visit::Continue);
		};

		let depth = name.matches('/').count();
		if name.rsplit('/').next() == Some("manifest.json")
			&& depth <= max_depth
			&& entry.size <= MAX_MANIFEST_SIZE
		{
			let mut data = Vec::new();
			reader.take(MAX_MANIFEST_SIZE).read_to_end(&mut data)?;
			manifests.insert(name.clone(), data);
		}

		files.insert(name);
		Ok(Visit::Continue)
	})?;

	let mut roots = BTreeSet::new();

	for name in &files {
		let (directory, file) = name.rsplit_once('/').unwrap_or(("", name.as_str()));

		if directory.matches('/').count() + usize::from(!directory.is_empty()) > max_depth
			|| directory.split('/').any(is_ignored_directory_name)
		{
			continue;
		}

		let is_pack = match file {
			"pack.mcmeta" => {
				let assets = if directory.is_empty() {
					"assets/".to_string()
				} else {
					format!("{directory}/assets/")
				};
				let data = assets.replace("assets/", "data/");

				files
					.range(assets.clone()..)
					.next()
					.is_some_and(|f| f.starts_with(&assets))
					|| files
						.range(data.clone()..)
						.next()
						.is_some_and(|f| f.starts_with(&data))
			}
			"manifest.json" => manifests
				.get(name)
				.is_some_and(|data| is_bedrock_resource_manifest(data)),
			_ => false
		};

		if is_pack {
			roots.insert(directory.to_string());
		}
	}

	// A pack inside another pack is part of that pack
	let all_roots: Vec<_> = roots.iter().cloned().collect();
	let pack_roots = all_roots
		.iter()
		.filter(|root| {
			!all_roots.iter().any(|other| {
				other != *root && (other.is_empty() || root.starts_with(&format!("{other}/")))
			})
		})
		.cloned()
		.collect();

	detail!(
		"Scanned the archive {} ({format:?}): {entries} entries, {} files, pack directories found: {:?}",
		path.display(),
		files.len(),
		pack_roots
	);

	Ok(Some(ArchiveScan {
		format,
		pack_roots: Some(pack_roots)
	}))
}

/// Packs that were extracted from an archive into a temporary directory, which is deleted when this is dropped.
#[derive(Debug)]
pub struct ExtractedPacks {
	_directory: tempfile::TempDir,
	/// The root of every pack: its directory in the archive (`""` for the root of the archive), and where it is now.
	pub packs: Vec<(String, PathBuf)>
}

/// Extracts the packs of a scanned archive into a new temporary directory.
pub fn extract_packs(
	archive: &Path,
	scan: &ArchiveScan,
	max_depth: usize,
	limits: ExtractLimits
) -> Result<ExtractedPacks, ArchiveError> {
	let directory = tempfile::Builder::new().prefix("packsquash-").tempdir()?;

	let roots: Vec<String> = match &scan.pack_roots {
		Some(roots) => {
			extract_archive(archive, scan.format, directory.path(), roots, limits)?;
			roots.clone()
		}
		None => {
			extract_with_helper(archive, directory.path())?;
			clean_helper_output(directory.path(), limits)?;
			find_pack_directories(directory.path(), max_depth)
				.into_iter()
				.filter_map(|path| {
					path.strip_prefix(directory.path())
						.ok()
						.map(|relative| relative.to_string_lossy().replace('\\', "/"))
				})
				.collect()
		}
	};

	let packs: Vec<_> = roots
		.into_iter()
		.map(|root| {
			let path = if root.is_empty() {
				directory.path().to_path_buf()
			} else {
				directory.path().join(root.split('/').collect::<PathBuf>())
			};
			(root, path)
		})
		.filter(|(_, path)| is_pack_directory(path))
		.collect();

	detail!(
		"Packs in the extracted archive {}, in {}: {:?}",
		archive.display(),
		directory.path().display(),
		packs
	);

	if packs.is_empty() {
		return Err(ArchiveError::NoPack);
	}

	Ok(ExtractedPacks {
		_directory: directory,
		packs
	})
}

#[cfg(test)]
mod tests;
