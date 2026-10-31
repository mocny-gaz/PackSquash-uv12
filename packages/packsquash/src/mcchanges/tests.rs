use std::fs;
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::{DeflateEncoder, ZlibEncoder};
use serde_json::json;
use tempfile::{Builder, TempDir};

use super::*;
use crate::config::{GlobalOptions, SquashOptions};
use crate::vfs::os_fs::OsFilesystem;
use crate::{PackSquasher, PackSquasherStatus, RelativePath};

pub(crate) fn temp_dir() -> TempDir {
	Builder::new()
		.prefix("ps-mcchanges-test")
		.tempdir()
		.expect("I/O operations are assumed not to fail during tests")
}

pub(crate) fn write_file(root: &Path, relative_path: &str, data: &[u8]) {
	let path = root.join(relative_path);
	fs::create_dir_all(path.parent().unwrap())
		.expect("I/O operations are assumed not to fail during tests");
	fs::write(path, data).expect("I/O operations are assumed not to fail during tests");
}

fn pack_file(root: &Path, relative_path: &str) -> VfsPackFileIterEntry {
	let file_path = root.join(relative_path);

	VfsPackFileIterEntry {
		relative_path: RelativePath::new(root, &file_path).unwrap().into_owned(),
		file_path
	}
}

/// Encodes an RGBA8 image as a PNG file, so tests can control how it is encoded.
pub(crate) fn png(width: u32, height: u32, rgba: &[u8], compression: Compression) -> Vec<u8> {
	fn chunk(out: &mut Vec<u8>, chunk_type: &[u8; 4], data: &[u8]) {
		let mut chunk_body = chunk_type.to_vec();
		chunk_body.extend_from_slice(data);

		out.extend_from_slice(&(data.len() as u32).to_be_bytes());
		out.extend_from_slice(&chunk_body);
		out.extend_from_slice(&crc32fast::hash(&chunk_body).to_be_bytes());
	}

	let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

	let mut header = Vec::new();
	header.extend_from_slice(&width.to_be_bytes());
	header.extend_from_slice(&height.to_be_bytes());
	header.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA, no interlacing
	chunk(&mut out, b"IHDR", &header);

	let mut scanlines = Vec::new();
	for row in rgba.chunks(width as usize * 4) {
		scanlines.push(0); // No filter
		scanlines.extend_from_slice(row);
	}

	let mut encoder = ZlibEncoder::new(Vec::new(), compression);
	encoder.write_all(&scanlines).unwrap();
	chunk(&mut out, b"IDAT", &encoder.finish().unwrap());
	chunk(&mut out, b"IEND", &[]);

	out
}

/// Builds a ZIP archive, whose entries are stored or deflated depending on their flag.
fn zip(entries: &[(&str, &[u8], bool)], comment: &[u8]) -> Vec<u8> {
	let mut archive = Vec::new();
	let mut central_directory = Vec::new();

	for (name, data, deflate) in entries {
		let (method, body) = if *deflate {
			let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
			encoder.write_all(data).unwrap();
			(8_u16, encoder.finish().unwrap())
		} else {
			(0_u16, data.to_vec())
		};

		let crc32 = crc32fast::hash(data);
		let local_header_offset = archive.len() as u32;

		archive.extend_from_slice(&0x0403_4B50_u32.to_le_bytes());
		archive.extend_from_slice(&[20, 0, 0, 0]); // Version needed, flags
		archive.extend_from_slice(&method.to_le_bytes());
		archive.extend_from_slice(&[0; 4]); // Modification time and date
		archive.extend_from_slice(&crc32.to_le_bytes());
		archive.extend_from_slice(&(body.len() as u32).to_le_bytes());
		archive.extend_from_slice(&(data.len() as u32).to_le_bytes());
		archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
		archive.extend_from_slice(&[0, 0]); // Extra field length
		archive.extend_from_slice(name.as_bytes());
		archive.extend_from_slice(&body);

		central_directory.extend_from_slice(&0x0201_4B50_u32.to_le_bytes());
		central_directory.extend_from_slice(&[20, 0, 20, 0, 0, 0]); // Versions, flags
		central_directory.extend_from_slice(&method.to_le_bytes());
		central_directory.extend_from_slice(&[0; 4]); // Modification time and date
		central_directory.extend_from_slice(&crc32.to_le_bytes());
		central_directory.extend_from_slice(&(body.len() as u32).to_le_bytes());
		central_directory.extend_from_slice(&(data.len() as u32).to_le_bytes());
		central_directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
		central_directory.extend_from_slice(&[0; 6]); // Extra field, comment and disk number
		central_directory.extend_from_slice(&[0; 6]); // Internal and external attributes
		central_directory.extend_from_slice(&local_header_offset.to_le_bytes());
		central_directory.extend_from_slice(name.as_bytes());
	}

	let central_directory_offset = archive.len() as u32;
	archive.extend_from_slice(&central_directory);

	archive.extend_from_slice(&0x0605_4B50_u32.to_le_bytes());
	archive.extend_from_slice(&[0; 4]); // Disk numbers
	archive.extend_from_slice(&(entries.len() as u16).to_le_bytes());
	archive.extend_from_slice(&(entries.len() as u16).to_le_bytes());
	archive.extend_from_slice(&(central_directory.len() as u32).to_le_bytes());
	archive.extend_from_slice(&central_directory_offset.to_le_bytes());
	archive.extend_from_slice(&(comment.len() as u16).to_le_bytes());
	archive.extend_from_slice(comment);

	archive
}

async fn mcchanges_directory(directory: &TempDir) -> McChanges {
	McChanges::open(&OsFilesystem, directory.path(), None)
		.await
		.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn identical_files_are_redundant() {
	let (mcchanges_dir, pack_dir) = (temp_dir(), temp_dir());
	let file = "assets/minecraft/lang/en_us.lang";

	write_file(mcchanges_dir.path(), file, b"a=b\n");
	write_file(pack_dir.path(), file, b"a=b\n");
	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/lang/es_es.lang",
		b"a=b\n"
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/lang/es_es.lang",
		b"a=c\n"
	);
	write_file(pack_dir.path(), "assets/minecraft/lang/new.lang", b"a=b\n");

	let mcchanges = mcchanges_directory(&mcchanges_dir).await;
	let is_redundant = |file: &str| {
		let pack_file = pack_file(pack_dir.path(), file);
		let mcchanges = &mcchanges;
		async move { mcchanges.is_redundant(&OsFilesystem, &pack_file).await }
	};

	assert!(is_redundant(file).await);
	assert!(!is_redundant("assets/minecraft/lang/es_es.lang").await);
	assert!(!is_redundant("assets/minecraft/lang/new.lang").await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn pack_metadata_is_never_redundant() {
	let (mcchanges_dir, pack_dir) = (temp_dir(), temp_dir());

	for file in ["pack.mcmeta", "pack.png"] {
		write_file(mcchanges_dir.path(), file, b"same");
		write_file(pack_dir.path(), file, b"same");
	}

	let mcchanges = mcchanges_directory(&mcchanges_dir).await;

	for file in ["pack.mcmeta", "pack.png"] {
		assert!(
			!mcchanges
				.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), file))
				.await
		);
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn json_files_are_compared_by_contents() {
	let (mcchanges_dir, pack_dir) = (temp_dir(), temp_dir());

	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/models/item/a.json",
		br#"{"parent": "item/generated", "textures": {"layer0": "items/a"}}"#
	);
	// Same contents, but minified, with another key order, and with a BOM
	write_file(
		pack_dir.path(),
		"assets/minecraft/models/item/a.json",
		b"\xEF\xBB\xBF{\"textures\":{\"layer0\":\"items/a\"},\"parent\":\"item/generated\"}"
	);
	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/models/item/b.json",
		br#"{"parent": "item/generated"}"#
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/models/item/b.json",
		br#"{"parent": "item/handheld"}"#
	);

	let mcchanges = mcchanges_directory(&mcchanges_dir).await;

	assert!(
		mcchanges
			.is_redundant(
				&OsFilesystem,
				&pack_file(pack_dir.path(), "assets/minecraft/models/item/a.json")
			)
			.await
	);
	assert!(
		!mcchanges
			.is_redundant(
				&OsFilesystem,
				&pack_file(pack_dir.path(), "assets/minecraft/models/item/b.json")
			)
			.await
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn textures_are_compared_by_pixels() {
	let (mcchanges_dir, pack_dir) = (temp_dir(), temp_dir());

	let mcchanges_pixels = [
		1, 2, 3, 255, 0, 0, 0, 0, //
		9, 8, 7, 255, 4, 5, 6, 128
	];
	// The color of the fully transparent pixel is not visible
	let mut same_pixels = mcchanges_pixels;
	same_pixels[4..7].copy_from_slice(&[10, 20, 30]);
	let mut other_pixels = mcchanges_pixels;
	other_pixels[0] = 200;

	let mcchanges_png = png(2, 2, &mcchanges_pixels, Compression::best());
	let reencoded_png = png(2, 2, &same_pixels, Compression::none());
	assert_ne!(mcchanges_png, reencoded_png);

	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/textures/a.png",
		&mcchanges_png
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/a.png",
		&reencoded_png
	);
	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/textures/b.png",
		&mcchanges_png
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/b.png",
		&png(2, 2, &other_pixels, Compression::best())
	);
	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/textures/c.png",
		&mcchanges_png
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/c.png",
		&png(4, 1, &mcchanges_pixels, Compression::best())
	);
	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/textures/d.png",
		&mcchanges_png
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/d.png",
		b"not a PNG"
	);

	let mcchanges = mcchanges_directory(&mcchanges_dir).await;
	let is_redundant = |name: &str| {
		let pack_file = pack_file(
			pack_dir.path(),
			&format!("assets/minecraft/textures/{name}.png")
		);
		let mcchanges = &mcchanges;
		async move { mcchanges.is_redundant(&OsFilesystem, &pack_file).await }
	};

	assert!(is_redundant("a").await, "re-encoded texture");
	assert!(!is_redundant("b").await, "different pixels");
	assert!(!is_redundant("c").await, "different size");
	assert!(!is_redundant("d").await, "invalid PNG");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn textures_and_their_animation_metadata_are_only_dropped_together() {
	let texture = png(1, 1, &[1, 2, 3, 255], Compression::best());
	let other_texture = png(1, 1, &[9, 9, 9, 255], Compression::best());
	let metadata = br#"{"animation": {"frametime": 2}}"#;
	let reformatted_metadata = b"{\"animation\":{\"frametime\":2}}";
	let other_metadata = br#"{"animation": {"frametime": 5}}"#;

	// (pack texture, pack metadata, mcchanges metadata, are the texture and metadata redundant)
	type TextureCase<'a> = (&'a [u8], Option<&'a [u8]>, Option<&'a [u8]>, bool);
	let cases: [TextureCase; 6] = [
		(&texture, Some(reformatted_metadata), Some(metadata), true),
		(&texture, None, None, true),
		(&texture, Some(other_metadata), Some(metadata), false),
		(&other_texture, Some(metadata), Some(metadata), false),
		(&texture, None, Some(metadata), false),
		(&texture, Some(metadata), None, false)
	];

	for (case, (pack_texture, pack_metadata, mcchanges_metadata, expected)) in cases.iter().enumerate()
	{
		let (mcchanges_dir, pack_dir) = (temp_dir(), temp_dir());
		let texture_path = "assets/minecraft/textures/blocks/lava_flow.png";

		write_file(mcchanges_dir.path(), texture_path, &texture);
		write_file(pack_dir.path(), texture_path, pack_texture);

		if let Some(metadata) = mcchanges_metadata {
			write_file(
				mcchanges_dir.path(),
				&format!("{texture_path}.mcmeta"),
				metadata
			);
		}
		if let Some(metadata) = pack_metadata {
			write_file(pack_dir.path(), &format!("{texture_path}.mcmeta"), metadata);
		}

		let mcchanges = mcchanges_directory(&mcchanges_dir).await;

		assert_eq!(
			mcchanges
				.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), texture_path))
				.await,
			*expected,
			"texture of case {case}"
		);

		if pack_metadata.is_some() {
			assert_eq!(
				mcchanges
					.is_redundant(
						&OsFilesystem,
						&pack_file(pack_dir.path(), &format!("{texture_path}.mcmeta"))
					)
					.await,
				*expected,
				"metadata of case {case}"
			);
		}
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn archives_can_be_mcchanges() {
	let (archive_dir, pack_dir) = (temp_dir(), temp_dir());
	let texture = png(1, 1, &[1, 2, 3, 255], Compression::best());

	let archive = zip(
		&[
			("assets/minecraft/", b"", false),
			("assets/minecraft/lang/en_us.lang", b"a=b\n", true),
			("assets/minecraft/textures/a.png", &texture, true),
			("assets/minecraft/textures/a.png.mcmeta", b"{}", false)
		],
		b"a comment that is longer than nothing"
	);
	write_file(archive_dir.path(), "client.jar", &archive);

	write_file(
		pack_dir.path(),
		"assets/minecraft/lang/en_us.lang",
		b"a=b\n"
	);
	write_file(pack_dir.path(), "assets/minecraft/textures/a.png", &texture);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/a.png.mcmeta",
		b"{ }"
	);
	write_file(pack_dir.path(), "assets/minecraft/textures/b.png", &texture);

	let mcchanges = McChanges::open(&OsFilesystem, &archive_dir.path().join("client.jar"), None)
		.await
		.unwrap();

	for (file, expected) in [
		("assets/minecraft/lang/en_us.lang", true),
		("assets/minecraft/textures/a.png", true),
		("assets/minecraft/textures/a.png.mcmeta", true),
		("assets/minecraft/textures/b.png", false)
	] {
		assert_eq!(
			mcchanges
				.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), file))
				.await,
			expected,
			"{file}"
		);
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn unusable_mcchanges_are_rejected() {
	let dir = temp_dir();
	write_file(dir.path(), "not_a_zip.jar", b"definitely not a ZIP file");

	assert!(matches!(
		McChanges::open(&OsFilesystem, &dir.path().join("not_a_zip.jar"), None).await,
		Err(McChangesError::InvalidArchive(_))
	));
	assert!(matches!(
		McChanges::open(&OsFilesystem, &dir.path().join("missing"), None).await,
		Err(McChangesError::IoError(_))
	));
}

#[test]
fn zip_reader_reads_stored_and_deflated_entries() {
	let big_entry = vec![b'x'; 100_000];
	let archive = ZipArchive::parse(zip(
		&[
			("stored.txt", b"stored data", false),
			("deflated.txt", b"deflated data", true),
			("empty.txt", b"", false),
			("big.txt", &big_entry, true),
			("folder/", b"", false)
		],
		b""
	))
	.unwrap();

	assert_eq!(
		archive.read("stored.txt").as_deref(),
		Some(&b"stored data"[..])
	);
	assert_eq!(
		archive.read("deflated.txt").as_deref(),
		Some(&b"deflated data"[..])
	);
	assert_eq!(archive.read("empty.txt").as_deref(), Some(&b""[..]));
	assert_eq!(archive.read("big.txt"), Some(big_entry));
	assert_eq!(archive.read("missing.txt"), None);
	assert!(!archive.contains("folder/"), "directories are not files");
}

#[test]
fn zip_reader_rejects_corrupt_data() {
	let valid = zip(&[("a.txt", b"hello world", false)], b"");

	// Not an archive, or one truncated in the middle of its central directory
	assert!(matches!(
		ZipArchive::parse(b"abc".to_vec()),
		Err(ZipReadError::MissingEndOfCentralDirectory)
	));
	assert!(ZipArchive::parse(valid[..valid.len() - 30].to_vec()).is_err());

	// A flipped bit in the file data must be caught by the CRC check
	let mut corrupt = valid.clone();
	corrupt[40] ^= 1;
	assert_eq!(ZipArchive::parse(corrupt).unwrap().read("a.txt"), None);

	// Unsupported compression methods
	let mut unsupported = valid;
	let central_directory_offset = unsupported
		.windows(4)
		.position(|window| window == 0x0201_4B50_u32.to_le_bytes())
		.unwrap();
	unsupported[central_directory_offset + 10] = 12; // BZIP2
	assert_eq!(ZipArchive::parse(unsupported).unwrap().read("a.txt"), None);
}

/// Not async because `PackSquasher::run` starts its own runtime.
#[test]
fn squashing_leaves_out_files_identical_to_the_mcchanges() {
	let (mcchanges_dir, pack_dir, output_dir) = (temp_dir(), temp_dir(), temp_dir());

	let stone = png(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255], Compression::best());
	let dirt = png(2, 1, &[7, 7, 7, 255, 8, 8, 8, 255], Compression::best());
	let modified_dirt = png(2, 1, &[7, 7, 7, 255, 9, 9, 9, 255], Compression::best());

	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/textures/blocks/stone.png",
		&stone
	);
	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/textures/blocks/dirt.png",
		&dirt
	);

	write_file(
		pack_dir.path(),
		"pack.mcmeta",
		br#"{"pack": {"pack_format": 1, "description": "Test pack"}}"#
	);
	// Re-encoded, but with the same pixels as the mcchanges
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/blocks/stone.png",
		&png(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255], Compression::none())
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/blocks/dirt.png",
		&modified_dirt
	);

	let output_path = output_dir.path().join("pack.zip");
	let mut global_options = GlobalOptions {
		mcchanges_path: Some(mcchanges_dir.path().to_path_buf()),
		output_file_path: output_path.clone(),
		..GlobalOptions::default()
	};
	global_options.zip_compression_iterations = 1;

	let (sender, mut receiver) = tokio::sync::mpsc::channel(64);

	PackSquasher::new()
		.run(
			OsFilesystem,
			SquashOptions {
				pack_directory: pack_dir.path().to_path_buf(),
				global_options,
				file_options: Default::default()
			},
			Some(sender)
		)
		.expect("Squashing a valid pack should succeed");

	let mut skipped_due_to_mcchanges = Vec::new();
	while let Ok(status) = receiver.try_recv() {
		if let PackSquasherStatus::PackFileProcessed(status) = status {
			assert!(status.optimization_error().is_none());

			if status.intentionally_skipped() {
				assert!(status.skipped());
				skipped_due_to_mcchanges.push(status.path().as_str().to_owned());
			}
		}
	}
	assert_eq!(
		skipped_due_to_mcchanges,
		["assets/minecraft/textures/blocks/stone.png"]
	);

	let output = ZipArchive::parse(fs::read(output_path).unwrap()).unwrap();
	assert!(output.contains("pack.mcmeta"));
	assert!(output.contains("assets/minecraft/textures/blocks/dirt.png"));
	assert!(!output.contains("assets/minecraft/textures/blocks/stone.png"));
}

fn language_entry(root: &Path, file: &str, pack_data: &[u8], mcchanges_data: &[u8]) -> (TempDir, VfsPackFileIterEntry) {
	let mcchanges_dir = temp_dir();
	write_file(mcchanges_dir.path(), file, mcchanges_data);
	write_file(root, file, pack_data);

	(mcchanges_dir, pack_file(root, file))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn json_language_keys_identical_to_the_mcchanges_are_removed() {
	let file = "assets/minecraft/lang/es_es.json";
	let mcchanges = br#"{"a": "1", "b": "2", "c": "3"}"#;

	let reduce = |pack: &'static [u8]| async move {
		let pack_dir = temp_dir();
		let (mcchanges_dir, entry) = language_entry(pack_dir.path(), file, pack, mcchanges);
		mcchanges_directory(&mcchanges_dir)
			.await
			.reduce_language_file(&OsFilesystem, &entry)
			.await
	};

	// Same key with another value, and a new key, are kept. Order is preserved
	let LanguageFileReduction::Reduced { data } =
		reduce(br#"{"z": "26", "a": "1", "b": "changed", "d": "4"}"#).await
	else {
		panic!("Some keys should have been removed");
	};
	assert_eq!(
		String::from_utf8(data).unwrap(),
		r#"{"z":"26","b":"changed","d":"4"}"#
	);

	assert_eq!(
		reduce(br#"{"a": "1", "c": "3"}"#).await,
		LanguageFileReduction::Redundant
	);
	assert_eq!(
		reduce(br#"{"a": "other", "e": "5"}"#).await,
		LanguageFileReduction::Unchanged
	);
	assert_eq!(reduce(b"not JSON").await, LanguageFileReduction::Unchanged);
	assert_eq!(reduce(b"[1, 2]").await, LanguageFileReduction::Unchanged);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn legacy_language_keys_identical_to_the_mcchanges_are_removed() {
	let file = "assets/minecraft/lang/en_US.lang";
	let mcchanges = b"a=1\nb=2\nc=3 \nd=\n";

	let reduce = |pack: &'static [u8]| async move {
		let pack_dir = temp_dir();
		let (mcchanges_dir, entry) = language_entry(pack_dir.path(), file, pack, mcchanges);
		mcchanges_directory(&mcchanges_dir)
			.await
			.reduce_language_file(&OsFilesystem, &entry)
			.await
	};
	let reduced = |reduction: LanguageFileReduction| match reduction {
		LanguageFileReduction::Reduced { data } => String::from_utf8(data).unwrap(),
		other => panic!("Some keys should have been removed, but got {other:?}")
	};

	// Comments, blank lines and line endings are kept. Values are not trimmed, so "c=3"
	// differs from "c=3 ". An empty value is a value like any other
	assert_eq!(
		reduced(reduce(b"# Comment\r\na=1\r\n\r\nb=changed\r\nc=3\r\nd=\r\ne=5").await),
		"# Comment\r\n\r\nb=changed\r\nc=3\r\ne=5"
	);

	// The game uses the last value of repeated keys, so that is the one that counts
	assert_eq!(
		reduced(reduce(b"a=changed\na=1\nb=changed\nb=2\nb=changed again\nz=26\n").await),
		"b=changed\nb=2\nb=changed again\nz=26\n"
	);

	assert_eq!(
		reduce(b"# Only inherited\na=1\nb=2\n").await,
		LanguageFileReduction::Redundant
	);
	assert_eq!(
		reduce(b"a=changed\nnew=1\n").await,
		LanguageFileReduction::Unchanged
	);
	assert_eq!(reduce(b"\xFF\xFE").await, LanguageFileReduction::Unchanged);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn language_files_are_only_reduced_when_they_are_language_files() {
	let (mcchanges_dir, pack_dir) = (temp_dir(), temp_dir());

	for file in [
		"assets/minecraft/texts/credits.json",
		"assets/minecraft/lang/notes.txt",
		"lang/es_es.json"
	] {
		write_file(mcchanges_dir.path(), file, br#"{"a": "1"}"#);
		write_file(pack_dir.path(), file, br#"{"a": "1", "b": "2"}"#);
	}

	let mcchanges = mcchanges_directory(&mcchanges_dir).await;

	for file in [
		"assets/minecraft/texts/credits.json",
		"assets/minecraft/lang/notes.txt",
		"lang/es_es.json"
	] {
		assert_eq!(
			mcchanges
				.reduce_language_file(&OsFilesystem, &pack_file(pack_dir.path(), file))
				.await,
			LanguageFileReduction::Unchanged,
			"{file}"
		);
	}
}

fn versioned_jar(index: &str) -> Vec<u8> {
	zip(
		&[
			("mcchanges.json", index.as_bytes(), true),
			("1.0/assets/minecraft/texts/a.txt", b"a from 1.0", true),
			("1.0/assets/minecraft/texts/b.txt", b"b from 1.0", true),
			("1.1/assets/minecraft/texts/a.txt", b"a from 1.1", true),
			("1.2/assets/minecraft/texts/c.txt", b"c from 1.2", true)
		],
		b""
	)
}

const VERSION_INDEX: &str = r#"{
	"format": 1,
	"versions": ["1.0", "1.1", "1.2"],
	"removed": {"1.2": ["assets/minecraft/texts/b.txt"]}
}"#;

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn versioned_mcchanges_provide_the_files_of_the_selected_version() {
	let (jar_dir, pack_dir) = (temp_dir(), temp_dir());
	write_file(jar_dir.path(), "vanilla.jar", &versioned_jar(VERSION_INDEX));

	for (name, content) in [
		("a", "a from 1.0"),
		("b", "b from 1.0"),
		("c", "c from 1.2")
	] {
		write_file(pack_dir.path(), &format!("assets/minecraft/texts/{name}.txt"), content.as_bytes());
	}
	write_file(pack_dir.path(), "assets/minecraft/texts/a_1_1.txt", b"unused");
	write_file(pack_dir.path(), "assets/minecraft/texts/a.txt.1_1", b"unused");

	// For each version, whether the pack files named a, b and c are redundant
	for (version, expected) in [
		("1.0", [true, true, false]),
		("1.1", [false, true, false]),
		("1.2", [false, false, true])
	] {
		let mcchanges = McChanges::open(
			&OsFilesystem,
			&jar_dir.path().join("vanilla.jar"),
			Some(version)
		)
		.await
		.unwrap();

		for (name, expected) in ["a", "b", "c"].into_iter().zip(expected) {
			assert_eq!(
				mcchanges
					.is_redundant(
						&OsFilesystem,
						&pack_file(pack_dir.path(), &format!("assets/minecraft/texts/{name}.txt"))
					)
					.await,
				expected,
				"file {name} in version {version}"
			);
		}
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn versioned_mcchanges_can_be_directories() {
	let mcchanges_dir = temp_dir();
	write_file(mcchanges_dir.path(), "mcchanges.json", VERSION_INDEX.as_bytes());
	write_file(mcchanges_dir.path(), "1.0/assets/minecraft/texts/a.txt", b"a from 1.0");
	write_file(mcchanges_dir.path(), "1.1/assets/minecraft/texts/a.txt", b"a from 1.1");

	let pack_dir = temp_dir();
	write_file(pack_dir.path(), "assets/minecraft/texts/a.txt", b"a from 1.1");

	let mcchanges = McChanges::open(&OsFilesystem, mcchanges_dir.path(), Some("1.2"))
		.await
		.unwrap();

	assert!(
		mcchanges
			.is_redundant(
				&OsFilesystem,
				&pack_file(pack_dir.path(), "assets/minecraft/texts/a.txt")
			)
			.await
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn versioned_mcchanges_reject_wrong_version_selections() {
	let dir = temp_dir();
	write_file(dir.path(), "vanilla.jar", &versioned_jar(VERSION_INDEX));
	write_file(dir.path(), "plain.jar", &zip(&[("assets/a.txt", b"a", false)], b""));
	write_file(
		dir.path(),
		"bad_format.jar",
		&versioned_jar(r#"{"format": 2, "versions": ["1.0"]}"#)
	);
	write_file(dir.path(), "bad_index.jar", &versioned_jar("not JSON"));

	let open = |file: &'static str, version: Option<&'static str>| {
		let path = dir.path().join(file);
		async move { McChanges::open(&OsFilesystem, &path, version).await.err() }
	};

	assert!(matches!(
		open("vanilla.jar", None).await,
		Some(McChangesError::VersionRequired(versions)) if versions == "1.0, 1.1, 1.2"
	));
	assert!(matches!(
		open("vanilla.jar", Some("9.9")).await,
		Some(McChangesError::UnknownVersion { version, .. }) if version == "9.9"
	));
	assert!(matches!(
		open("plain.jar", Some("1.0")).await,
		Some(McChangesError::NotVersioned)
	));
	assert!(matches!(
		open("bad_format.jar", Some("1.0")).await,
		Some(McChangesError::InvalidVersionIndex(_))
	));
	assert!(matches!(
		open("bad_index.jar", Some("1.0")).await,
		Some(McChangesError::InvalidVersionIndex(_))
	));
	assert!(open("plain.jar", None).await.is_none());
}

/// Not async because `PackSquasher::run` starts its own runtime.
#[test]
fn squashing_removes_language_keys_identical_to_the_mcchanges() {
	for skip_language_keys in [true, false] {
		let (mcchanges_dir, pack_dir, output_dir) = (temp_dir(), temp_dir(), temp_dir());

		write_file(mcchanges_dir.path(), "assets/minecraft/lang/en_US.lang", b"a=1\nb=2\nc=3\n");
		write_file(mcchanges_dir.path(), "assets/minecraft/lang/fr_FR.lang", b"a=1\nb=2\n");

		write_file(
			pack_dir.path(),
			"pack.mcmeta",
			br#"{"pack": {"pack_format": 1, "description": "Test pack"}}"#
		);
		write_file(pack_dir.path(), "assets/minecraft/lang/en_US.lang", b"a=1\nb=9\nc=3\nd=4\n");
		write_file(pack_dir.path(), "assets/minecraft/lang/fr_FR.lang", b"a=1\n");

		let output_path = output_dir.path().join("pack.zip");
		let mut global_options = GlobalOptions {
			mcchanges_path: Some(mcchanges_dir.path().to_path_buf()),
			skip_language_keys_identical_to_mcchanges: skip_language_keys,
			output_file_path: output_path.clone(),
			..GlobalOptions::default()
		};
		global_options.zip_compression_iterations = 1;

		let (sender, mut receiver) = tokio::sync::mpsc::channel(64);

		PackSquasher::new()
			.run(
				OsFilesystem,
				SquashOptions {
					pack_directory: pack_dir.path().to_path_buf(),
					global_options,
					file_options: Default::default()
				},
				Some(sender)
			)
			.expect("Squashing a valid pack should succeed");

		let mut intentionally_skipped = Vec::new();
		while let Ok(status) = receiver.try_recv() {
			if let PackSquasherStatus::PackFileProcessed(status) = status {
				assert!(status.optimization_error().is_none());

				if status.intentionally_skipped() {
					intentionally_skipped.push(status.path().as_str().to_owned());
				}
			}
		}

		let output = ZipArchive::parse(fs::read(output_path).unwrap()).unwrap();
		let english = String::from_utf8(output.read("assets/minecraft/lang/en_US.lang").unwrap()).unwrap();

		if skip_language_keys {
			assert_eq!(intentionally_skipped, ["assets/minecraft/lang/fr_FR.lang"]);
			assert!(output.read("assets/minecraft/lang/fr_FR.lang").is_none());
			assert!(english.contains("b=9") && english.contains("d=4"));
			assert!(!english.contains("a=1") && !english.contains("c=3"), "{english}");
		} else {
			assert!(intentionally_skipped.is_empty());
			assert!(output.read("assets/minecraft/lang/fr_FR.lang").is_some());
			assert!(english.contains("a=1") && english.contains("c=3"));
		}
	}
}

/// Turns an archive that has no comment into one that has the fields of its end of central
/// directory record maxed out, and the real values in ZIP64 records.
fn with_zip64_end_records(archive: &[u8]) -> Vec<u8> {
	let end = archive.len() - 22;
	let entries = u16::from_le_bytes(archive[end + 10..end + 12].try_into().unwrap());
	let size = u32::from_le_bytes(archive[end + 12..end + 16].try_into().unwrap());
	let offset = u32::from_le_bytes(archive[end + 16..end + 20].try_into().unwrap());

	let mut out = archive[..end].to_vec();
	let record_offset = out.len() as u64;

	out.extend_from_slice(&0x0606_4B50_u32.to_le_bytes());
	out.extend_from_slice(&44_u64.to_le_bytes());
	out.extend_from_slice(&[45, 0, 45, 0]); // Versions
	out.extend_from_slice(&[0; 8]); // Disk numbers
	out.extend_from_slice(&u64::from(entries).to_le_bytes());
	out.extend_from_slice(&u64::from(entries).to_le_bytes());
	out.extend_from_slice(&u64::from(size).to_le_bytes());
	out.extend_from_slice(&u64::from(offset).to_le_bytes());

	out.extend_from_slice(&0x0706_4B50_u32.to_le_bytes());
	out.extend_from_slice(&0_u32.to_le_bytes());
	out.extend_from_slice(&record_offset.to_le_bytes());
	out.extend_from_slice(&1_u32.to_le_bytes());

	out.extend_from_slice(&0x0605_4B50_u32.to_le_bytes());
	out.extend_from_slice(&[0; 4]);
	out.extend_from_slice(&u16::MAX.to_le_bytes());
	out.extend_from_slice(&u16::MAX.to_le_bytes());
	out.extend_from_slice(&u32::MAX.to_le_bytes());
	out.extend_from_slice(&u32::MAX.to_le_bytes());
	out.extend_from_slice(&0_u16.to_le_bytes());

	out
}

/// Builds an archive with a single stored entry whose sizes and offset are in a ZIP64 extra field.
fn zip64_entry_archive(name: &str, data: &[u8]) -> Vec<u8> {
	let crc32 = crc32fast::hash(data);
	let mut archive = Vec::new();

	archive.extend_from_slice(&0x0403_4B50_u32.to_le_bytes());
	archive.extend_from_slice(&[45, 0, 0, 0, 0, 0]); // Version, flags, method
	archive.extend_from_slice(&[0; 4]);
	archive.extend_from_slice(&crc32.to_le_bytes());
	archive.extend_from_slice(&(data.len() as u32).to_le_bytes());
	archive.extend_from_slice(&(data.len() as u32).to_le_bytes());
	archive.extend_from_slice(&(name.len() as u16).to_le_bytes());
	archive.extend_from_slice(&[0, 0]);
	archive.extend_from_slice(name.as_bytes());
	archive.extend_from_slice(data);

	let mut central_directory = Vec::new();
	central_directory.extend_from_slice(&0x0201_4B50_u32.to_le_bytes());
	central_directory.extend_from_slice(&[45, 0, 45, 0, 0, 0, 0, 0]); // Versions, flags, method
	central_directory.extend_from_slice(&[0; 4]);
	central_directory.extend_from_slice(&crc32.to_le_bytes());
	central_directory.extend_from_slice(&u32::MAX.to_le_bytes());
	central_directory.extend_from_slice(&u32::MAX.to_le_bytes());
	central_directory.extend_from_slice(&(name.len() as u16).to_le_bytes());
	central_directory.extend_from_slice(&32_u16.to_le_bytes()); // Extra field length
	central_directory.extend_from_slice(&[0; 10]); // Comment length, disk, attributes
	central_directory.extend_from_slice(&u32::MAX.to_le_bytes());
	central_directory.extend_from_slice(name.as_bytes());
	// An unrelated extra field, and then the ZIP64 one
	central_directory.extend_from_slice(&[0xFF, 0xFF, 0, 0]);
	central_directory.extend_from_slice(&1_u16.to_le_bytes());
	central_directory.extend_from_slice(&24_u16.to_le_bytes());
	central_directory.extend_from_slice(&(data.len() as u64).to_le_bytes());
	central_directory.extend_from_slice(&(data.len() as u64).to_le_bytes());
	central_directory.extend_from_slice(&0_u64.to_le_bytes());

	let central_directory_offset = archive.len() as u32;
	archive.extend_from_slice(&central_directory);

	archive.extend_from_slice(&0x0605_4B50_u32.to_le_bytes());
	archive.extend_from_slice(&[0; 4]);
	archive.extend_from_slice(&1_u16.to_le_bytes());
	archive.extend_from_slice(&1_u16.to_le_bytes());
	archive.extend_from_slice(&(central_directory.len() as u32).to_le_bytes());
	archive.extend_from_slice(&central_directory_offset.to_le_bytes());
	archive.extend_from_slice(&0_u16.to_le_bytes());

	archive
}

#[test]
fn zip_reader_supports_zip64_archives() {
	let archive = ZipArchive::parse(with_zip64_end_records(&zip(
		&[("a.txt", b"first", false), ("b.txt", b"second", true)],
		b""
	)))
	.unwrap();
	assert_eq!(archive.read("a.txt").as_deref(), Some(&b"first"[..]));
	assert_eq!(archive.read("b.txt").as_deref(), Some(&b"second"[..]));

	let archive = ZipArchive::parse(zip64_entry_archive("big.txt", b"zip64 entry")).unwrap();
	assert_eq!(archive.read("big.txt").as_deref(), Some(&b"zip64 entry"[..]));

	// A maxed out field without ZIP64 records can't be trusted
	let mut broken = with_zip64_end_records(&zip(&[("a.txt", b"first", false)], b""));
	let record_start = broken.len() - 22 - 20 - 56;
	broken[record_start] ^= 0xFF;
	assert!(matches!(
		ZipArchive::parse(broken),
		Err(ZipReadError::Zip64Unsupported)
	));
}

#[test]
fn zip_reader_handles_many_entries() {
	// More entries than the 16-bit counter of regular archives can hold
	let names = (0..70_000).map(|i| format!("f/{i}")).collect::<Vec<_>>();
	let entries = names
		.iter()
		.map(|name| (name.as_str(), &b"x"[..], false))
		.collect::<Vec<_>>();
	let archive = zip(&entries, b"");

	// The regular helper writes the wrapped counter, so convert it like a ZIP64 writer would
	let end = archive.len() - 22;
	let mut fixed = archive[..end].to_vec();
	fixed.extend_from_slice(&with_zip64_end_records(&{
		let mut tail = archive.clone();
		tail[end + 10..end + 12].copy_from_slice(&0_u16.to_le_bytes());
		tail
	})[end..]);
	// Patch the real counts into the ZIP64 record, which starts where the end record was
	fixed[end + 32..end + 40].copy_from_slice(&70_000_u64.to_le_bytes());
	fixed[end + 24..end + 32].copy_from_slice(&70_000_u64.to_le_bytes());

	let archive = ZipArchive::parse(fixed).unwrap();
	assert!(archive.contains("f/0") && archive.contains("f/69999"));
	assert_eq!(archive.read("f/65536").as_deref(), Some(&b"x"[..]));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn files_stored_as_hashes_are_compared_by_hash() {
	use sha1::Sha1;
	use sha2::{Digest, Sha256};

	let hexadecimal = |digest: &[u8]| {
		digest
			.iter()
			.map(|byte| format!("{byte:02x}"))
			.collect::<String>()
	};
	let sha1 = |data: &[u8]| format!("sha1:{}", hexadecimal(&Sha1::digest(data)));
	let sha256 = |data: &[u8]| format!("sha256:{}", hexadecimal(&Sha256::digest(data)).to_uppercase());

	let index = json!({
		"format": 1,
		"versions": ["1.0", "1.1", "1.2"],
		"hashed": {
			"1.0": {"assets/minecraft/sounds/b.ogg": sha256(b"sound b")},
			"1.1": {
				"assets/minecraft/sounds/a.ogg": sha1(b"sound a"),
				"assets/minecraft/sounds/c.ogg": "md5:0cc175b9c0f1b6a831c399e269772661",
				"assets/minecraft/sounds/d.ogg": "no algorithm"
			}
		},
		"removed": {"1.2": ["assets/minecraft/sounds/a.ogg"]}
	});

	let (dir, pack_dir, changed_pack_dir) = (temp_dir(), temp_dir(), temp_dir());
	write_file(
		dir.path(),
		"mcchanges.jar",
		&zip(
			&[
				("mcchanges.json", index.to_string().as_bytes(), true),
				("1.0/assets/minecraft/texts/a.txt", b"a text", true)
			],
			b""
		)
	);

	for (name, data) in [("a", "sound a"), ("b", "sound b"), ("c", "a"), ("d", "sound d")] {
		write_file(pack_dir.path(), &format!("assets/minecraft/sounds/{name}.ogg"), data.as_bytes());
		write_file(changed_pack_dir.path(), &format!("assets/minecraft/sounds/{name}.ogg"), b"changed");
	}

	// For each version, whether the files a, b, c and d are redundant
	for (version, expected) in [
		("1.0", [false, true, false, false]),
		("1.1", [true, true, false, false]),
		("1.2", [false, true, false, false])
	] {
		let mcchanges = McChanges::open(
			&OsFilesystem,
			&dir.path().join("mcchanges.jar"),
			Some(version)
		)
		.await
		.unwrap();

		for (name, expected) in ["a", "b", "c", "d"].into_iter().zip(expected) {
			let file = format!("assets/minecraft/sounds/{name}.ogg");

			assert_eq!(
				mcchanges
					.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), &file))
					.await,
				expected,
				"{name} in {version}"
			);
			assert!(
				!mcchanges
					.is_redundant(&OsFilesystem, &pack_file(changed_pack_dir.path(), &file))
					.await,
				"changed {name} in {version}"
			);
		}
	}
}

/// Tokens and key hashes below were computed by `build_mcchanges_jar.py`, so these tests check
/// that the build script and PackSquash agree. They are the pixels of a 2x2 image, whose third
/// pixel is fully transparent, and the canonical JSON of a document.
const HASHES_PIXELS: [u8; 16] = [255, 0, 0, 255, 0, 255, 0, 128, 9, 9, 9, 0, 1, 2, 3, 255];
const HASHES_PNG_TOKEN: &str = "png:2x2:f694ed61298646d789644a1250697762110dc2b61a986310f3abaa62601d6536";
const HASHES_JSON: &str = r#"{"b":[1,2.5,"é\n"],"a":{"z":null,"y":false}}"#;
const HASHES_JSON_TOKEN: &str = "json:ad4c63c14a6649b0b5574b6520785f1474ffe17a63b24f1964955a7fed25bfad";
/// The vanilla language file `{"a": "1", "b": "2", "c": "3"}`
const HASHES_LANG_JSON_TOKEN: &str = "langkeys:229e756964a795cbedd5d7d9eb5a3135ff7ca074260e0ad8427f79e7c2ebbb06";
const HASHES_LANG_JSON_KEYS: &str = "0a934f412f7d21f7b4366ca4a3d88d977a7b71a1db0c0517d2d8137896948869394000a0";
/// The vanilla language file `# c\na=1\nb=2\r\nc=3\na=1\n`
const HASHES_LANG_LEGACY: &[u8] = b"# c\na=1\nb=2\r\nc=3\na=1\n";
const HASHES_LANG_LEGACY_TOKEN: &str = "langkeys:15788b31caec836170a9a443c6dbaa484dcaa776fe45bfc5ed733ac92293d5d0";
const HASHES_LANG_LEGACY_KEYS: &str = "05a9c569a185ac60f5511f9768960076a8d49f6aa1a5890f76749254503e07c2a625e6b9";

fn from_hex(text: &str) -> Vec<u8> {
	(0..text.len())
		.step_by(2)
		.map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
		.collect()
}

/// A hashes jar with one version, `1.0`, that holds only the given tokens and extra files.
fn hashes_jar(hashed: &serde_json::Value, extra_files: &[(&str, Vec<u8>)]) -> Vec<u8> {
	let index = json!({"format": 1, "kind": "hashes", "versions": ["1.0"], "hashed": {"1.0": hashed}}).to_string();

	let mut entries: Vec<(&str, &[u8], bool)> = vec![("mcchanges.json", index.as_bytes(), true)];
	entries.extend(extra_files.iter().map(|(name, data)| (*name, data.as_slice(), true)));

	zip(&entries, b"")
}

async fn open_hashes_jar(directory: &TempDir, jar: &[u8]) -> McChanges {
	write_file(directory.path(), "hashes.jar", jar);

	McChanges::open(&OsFilesystem, &directory.path().join("hashes.jar"), Some("1.0"))
		.await
		.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn hashes_textures_are_compared_by_decoded_pixels() {
	let (jar_dir, pack_dir) = (temp_dir(), temp_dir());
	let jar = hashes_jar(
		&json!({
			"assets/minecraft/textures/a.png": HASHES_PNG_TOKEN,
			"assets/minecraft/textures/b.png": HASHES_PNG_TOKEN,
			"assets/minecraft/textures/c.png": HASHES_PNG_TOKEN,
			"assets/minecraft/textures/animated.png": HASHES_PNG_TOKEN,
			"assets/minecraft/textures/animated.png.mcmeta": HASHES_JSON_TOKEN,
			"assets/minecraft/textures/odd.png": format!("sha256:{}", hexadecimal(Sha256::digest(b"odd bytes")))
		}),
		&[]
	);
	let mcchanges = open_hashes_jar(&jar_dir, &jar).await;

	let mut changed_pixels = HASHES_PIXELS;
	changed_pixels[0] = 254;
	let mut other_transparent_color = HASHES_PIXELS;
	other_transparent_color[8..11].copy_from_slice(&[1, 2, 3]);

	write_file(pack_dir.path(), "assets/minecraft/textures/a.png", &png(2, 2, &HASHES_PIXELS, Compression::best()));
	write_file(pack_dir.path(), "assets/minecraft/textures/b.png", &png(2, 2, &other_transparent_color, Compression::none()));
	write_file(pack_dir.path(), "assets/minecraft/textures/c.png", &png(2, 2, &changed_pixels, Compression::best()));
	write_file(pack_dir.path(), "assets/minecraft/textures/odd.png", b"odd bytes");

	// A texture that has animation metadata is only the same if the metadata is the same
	write_file(pack_dir.path(), "assets/minecraft/textures/animated.png", &png(2, 2, &HASHES_PIXELS, Compression::none()));
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/animated.png.mcmeta",
		r#"{ "a": {"y": false, "z": null}, "b": [1, 2.50, "é\n"] }"#.as_bytes()
	);

	let is_redundant = |file: &str| {
		let pack_file = pack_file(pack_dir.path(), file);
		let mcchanges = &mcchanges;
		async move { mcchanges.is_redundant(&OsFilesystem, &pack_file).await }
	};

	assert!(is_redundant("assets/minecraft/textures/a.png").await);
	// b.png only differs in the color of a transparent pixel, which can't be seen
	assert!(is_redundant("assets/minecraft/textures/b.png").await);
	assert!(!is_redundant("assets/minecraft/textures/c.png").await);
	assert!(is_redundant("assets/minecraft/textures/odd.png").await);
	assert!(is_redundant("assets/minecraft/textures/animated.png").await);
	assert!(is_redundant("assets/minecraft/textures/animated.png.mcmeta").await);

	// The same pixels, but another size in the token
	let jar = hashes_jar(
		&json!({"assets/minecraft/textures/a.png": HASHES_PNG_TOKEN.replace("2x2", "4x1")}),
		&[]
	);
	let mcchanges = open_hashes_jar(&temp_dir(), &jar).await;
	assert!(
		!mcchanges
			.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), "assets/minecraft/textures/a.png"))
			.await
	);

	// The metadata changes
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/animated.png.mcmeta",
		r#"{"a":{"y":false,"z":null},"b":[1,2.6,"é\n"]}"#.as_bytes()
	);
	assert!(
		!open_hashes_jar(&temp_dir(), &hashes_jar(
			&json!({
				"assets/minecraft/textures/animated.png": HASHES_PNG_TOKEN,
				"assets/minecraft/textures/animated.png.mcmeta": HASHES_JSON_TOKEN
			}),
			&[]
		))
		.await
		.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), "assets/minecraft/textures/animated.png"))
		.await
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn hashes_json_files_are_compared_by_contents() {
	let (jar_dir, pack_dir) = (temp_dir(), temp_dir());
	let mcchanges = open_hashes_jar(
		&jar_dir,
		&hashes_jar(&json!({"assets/minecraft/models/m.json": HASHES_JSON_TOKEN}), &[])
	)
	.await;

	for (file, data, expected) in [
		("same.json", HASHES_JSON.as_bytes().to_vec(), true),
		("reformatted.json", "{\n \"a\": {\"y\": false, \"z\": null},\n \"b\": [1, 2.5, \"é\\n\"]\n}".into(), true),
		("bom.json", [b"\xEF\xBB\xBF".as_slice(), HASHES_JSON.as_bytes()].concat(), true),
		("other_value.json", HASHES_JSON.replace("2.5", "2.6").into_bytes(), false),
		("other_key.json", HASHES_JSON.replace("\"z\"", "\"x\"").into_bytes(), false),
		("integer.json", HASHES_JSON.replace("1,2.5", "1.0,2.5").into_bytes(), false),
		("invalid.json", b"{not json".to_vec(), false)
	] {
		// Every file takes the place of the vanilla one, in a pack of its own
		let pack_dir = if expected { &pack_dir } else { &temp_dir() };
		write_file(pack_dir.path(), "assets/minecraft/models/m.json", &data);

		assert_eq!(
			mcchanges
				.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), "assets/minecraft/models/m.json"))
				.await,
			expected,
			"{file}"
		);
	}
}

/// What reducing the language file gives for a pack language file, for a hashes jar and for the
/// vanilla files themselves, which have to agree.
async fn reductions(file: &str, token: &str, keys: &str, vanilla: &[u8], pack: &[u8]) -> (LanguageFileReduction, LanguageFileReduction) {
	let (jar_dir, vanilla_dir, pack_dir) = (temp_dir(), temp_dir(), temp_dir());
	write_file(vanilla_dir.path(), file, vanilla);
	write_file(pack_dir.path(), file, pack);

	let hashes_jar = open_hashes_jar(
		&jar_dir,
		&hashes_jar(&json!({ file: token }), &[(&format!("1.0/{file}.langkeys"), from_hex(keys))])
	)
	.await;
	let entry = pack_file(pack_dir.path(), file);

	(
		hashes_jar.reduce_language_file(&OsFilesystem, &entry).await,
		mcchanges_directory(&vanilla_dir)
			.await
			.reduce_language_file(&OsFilesystem, &entry)
			.await
	)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn hashes_json_language_keys_are_removed_like_with_the_files() {
	let file = "assets/minecraft/lang/es_es.json";
	let vanilla = br#"{"a": "1", "b": "2", "c": "3"}"#;

	for pack in [
		br#"{"z": "26", "a": "1", "b": "changed", "d": "4"}"#.as_slice(),
		br#"{"a": "1", "c": "3"}"#,
		br#"{"c":"3","a":"1","b":"2"}"#,
		br#"{"a": "other", "e": "5"}"#,
		br#"{"a": 1, "b": "2"}"#,
		b"not JSON",
		b"[1, 2]"
	] {
		let (from_hashes, files) = reductions(file, HASHES_LANG_JSON_TOKEN, HASHES_LANG_JSON_KEYS, vanilla, pack).await;
		assert_eq!(from_hashes, files, "{}", String::from_utf8_lossy(pack));
	}

	let (hashes_result, _) = reductions(file, HASHES_LANG_JSON_TOKEN, HASHES_LANG_JSON_KEYS, vanilla, br#"{"a": "1", "z": "26"}"#).await;
	assert_eq!(hashes_result, LanguageFileReduction::Reduced { data: br#"{"z":"26"}"#.to_vec() });
	let (hashes_result, _) = reductions(file, HASHES_LANG_JSON_TOKEN, HASHES_LANG_JSON_KEYS, vanilla, br#"{"a": "1", "c": "3"}"#).await;
	assert_eq!(hashes_result, LanguageFileReduction::Redundant);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn hashes_legacy_language_keys_are_removed_like_with_the_files() {
	let file = "assets/minecraft/lang/es_ES.lang";

	for pack in [
		b"a=1\nb=other\nd=4\n".as_slice(),
		b"a=2\na=1\n",
		b"a=1\na=2\n",
		b"# comment\r\na=1\r\n\r\nb=2\r\nc=3\r\n",
		b"a=1\nb=2\nc=4\n",
		b"b=2=2\n",
		b"\xFF\xFE=1\n"
	] {
		let (from_hashes, files) = reductions(file, HASHES_LANG_LEGACY_TOKEN, HASHES_LANG_LEGACY_KEYS, HASHES_LANG_LEGACY, pack).await;
		assert_eq!(from_hashes, files, "{}", String::from_utf8_lossy(pack));
	}

	let (hashes_result, _) = reductions(file, HASHES_LANG_LEGACY_TOKEN, HASHES_LANG_LEGACY_KEYS, HASHES_LANG_LEGACY, b"a=1\nb=other\nd=4\n").await;
	assert_eq!(hashes_result, LanguageFileReduction::Reduced { data: b"b=other\nd=4\n".to_vec() });
	let (hashes_result, _) = reductions(file, HASHES_LANG_LEGACY_TOKEN, HASHES_LANG_LEGACY_KEYS, HASHES_LANG_LEGACY, b"a=2\na=1\n").await;
	assert_eq!(hashes_result, LanguageFileReduction::Redundant);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn hashes_language_keys_that_do_not_match_their_token_are_not_used() {
	let file = "assets/minecraft/lang/es_es.json";
	let pack = br#"{"a": "1"}"#;
	let (mut tampered, mut truncated) = (from_hex(HASHES_LANG_JSON_KEYS), from_hex(HASHES_LANG_JSON_KEYS));
	tampered[0] ^= 1;
	truncated.pop();

	for keys in [tampered, truncated, Vec::new()] {
		let (jar_dir, pack_dir) = (temp_dir(), temp_dir());
		write_file(pack_dir.path(), file, pack);

		let hashes_jar = open_hashes_jar(
			&jar_dir,
			&hashes_jar(&json!({ file: HASHES_LANG_JSON_TOKEN }), &[(&format!("1.0/{file}.langkeys"), keys)])
		)
		.await;

		assert_eq!(
			hashes_jar.reduce_language_file(&OsFilesystem, &pack_file(pack_dir.path(), file)).await,
			LanguageFileReduction::Unchanged
		);
	}

	// A missing key file is the same
	let (jar_dir, pack_dir) = (temp_dir(), temp_dir());
	write_file(pack_dir.path(), file, pack);
	let hashes_jar = open_hashes_jar(&jar_dir, &hashes_jar(&json!({ file: HASHES_LANG_JSON_TOKEN }), &[])).await;
	assert_eq!(
		hashes_jar.reduce_language_file(&OsFilesystem, &pack_file(pack_dir.path(), file)).await,
		LanguageFileReduction::Unchanged
	);
	// An identical language file is not redundant by itself: the language key removal drops it
	assert!(!hashes_jar.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), file)).await);
}

/// Not async because `PackSquasher::run` starts its own runtime.
#[test]
fn squashing_with_a_hashes_jar_leaves_out_what_the_game_already_has() {
	let (jar_dir, pack_dir, output_dir) = (temp_dir(), temp_dir(), temp_dir());
	let language = "assets/minecraft/lang/en_us.json";

	write_file(
		jar_dir.path(),
		"hashes.jar",
		&hashes_jar(
			&json!({
				"assets/minecraft/textures/blocks/stone.png": HASHES_PNG_TOKEN,
				language: HASHES_LANG_JSON_TOKEN
			}),
			&[(&format!("1.0/{language}.langkeys"), from_hex(HASHES_LANG_JSON_KEYS))]
		)
	);

	let mut changed = HASHES_PIXELS;
	changed[0] = 100;

	write_file(
		pack_dir.path(),
		"pack.mcmeta",
		br#"{"pack": {"pack_format": 1, "description": "Test pack"}}"#
	);
	write_file(pack_dir.path(), "assets/minecraft/textures/blocks/stone.png", &png(2, 2, &HASHES_PIXELS, Compression::none()));
	write_file(pack_dir.path(), "assets/minecraft/textures/blocks/dirt.png", &png(2, 2, &changed, Compression::none()));
	write_file(pack_dir.path(), language, br#"{"a": "1", "b": "changed", "c": "3", "d": "4"}"#);

	let output_path = output_dir.path().join("pack.zip");
	let mut global_options = GlobalOptions {
		mcchanges_path: Some(jar_dir.path().join("hashes.jar")),
		mcchanges_version: Some("1.0".into()),
		output_file_path: output_path.clone(),
		..GlobalOptions::default()
	};
	global_options.zip_compression_iterations = 1;

	PackSquasher::new()
		.run(
			OsFilesystem,
			SquashOptions {
				pack_directory: pack_dir.path().to_path_buf(),
				global_options,
				file_options: Default::default()
			},
			None
		)
		.expect("Squashing a valid pack should succeed");

	let output = ZipArchive::parse(fs::read(output_path).unwrap()).unwrap();
	assert!(output.contains("pack.mcmeta"));
	assert!(!output.contains("assets/minecraft/textures/blocks/stone.png"));
	assert!(output.contains("assets/minecraft/textures/blocks/dirt.png"));

	let language = String::from_utf8(output.read(language).unwrap()).unwrap();
	assert!(language.contains("changed") && language.contains("\"d\""), "{language}");
	assert!(!language.contains("\"a\"") && !language.contains("\"c\""), "{language}");
}

/// A language file in three versions, with the compact tokens that `build_mcchanges_compact.py`
/// makes: 1.0 is `{"a": "1", "b": "2", "c": "3"}` (`langfull:`), 1.1 changes b, drops c and adds d
/// (`langdelta:`), and 1.2 changes a (`langdelta:`). The files and digests are computed in Python.
const COMPACT_PATH: &str = "assets/minecraft/lang/es_es.json";
const COMPACT_FULL: &str = "0a934f412f7d21f7a3d88d977a7b71a1d2d8137896948869";
const COMPACT_FULL_DIGEST: &str = "d5667f7ec050db21476ba6aca2c4923c46f8d01931fed26a8f1eaf08b692923c";
const COMPACT_DELTA_1: &str = "0200000096703a88476bd470a86f71ce026628900a934f412f7d21f7a3d88d977a7b71a1";
const COMPACT_DELTA_1_DIGEST: &str = "54f5a050d25af063ea91d852bd0b2c17aa657d8d1c5e2c18f968a54e7b0dd523";
const COMPACT_DELTA_2: &str = "0100000069603f60206d0f88d2d8137896948869";
const COMPACT_DELTA_2_DIGEST: &str = "9437d065f8dbb8285378d2b80143ad6d4d48e0a2e9ed601422ac157dcff4ba6a";

/// What reducing a pack language file gives, for a compact jar with the three versions.
/// `tamper_1_1` breaks the digest of the 1.1 key file, and `without_base` removes the `langfull:` file
/// of 1.0, so that the chain of changes has no start.
async fn compact_reduction(version: &str, pack: &[u8], tamper_1_1: bool, without_base: bool) -> LanguageFileReduction {
	let (jar_dir, pack_dir) = (temp_dir(), temp_dir());
	write_file(pack_dir.path(), COMPACT_PATH, pack);

	let mut delta_1 = from_hex(COMPACT_DELTA_1);
	if tamper_1_1 {
		delta_1.push(0);
	}

	let mut hashed = json!({
		"1.1": { COMPACT_PATH: format!("langdelta:{COMPACT_DELTA_1_DIGEST}") },
		"1.2": { COMPACT_PATH: format!("langdelta:{COMPACT_DELTA_2_DIGEST}") }
	});
	let mut files = vec![
		(format!("1.1/{COMPACT_PATH}.langkeys"), delta_1),
		(format!("1.2/{COMPACT_PATH}.langkeys"), from_hex(COMPACT_DELTA_2))
	];
	if !without_base {
		hashed["1.0"] = json!({ COMPACT_PATH: format!("langfull:{COMPACT_FULL_DIGEST}") });
		files.push((format!("1.0/{COMPACT_PATH}.langkeys"), from_hex(COMPACT_FULL)));
	}

	let index = json!({"format": 1, "kind": "hashes", "versions": ["1.0", "1.1", "1.2"], "hashed": hashed}).to_string();
	let mut entries: Vec<(&str, &[u8], bool)> = vec![("mcchanges.json", index.as_bytes(), true)];
	entries.extend(files.iter().map(|(name, data)| (name.as_str(), data.as_slice(), true)));
	write_file(jar_dir.path(), "compact.jar", &zip(&entries, b""));

	McChanges::open(&OsFilesystem, &jar_dir.path().join("compact.jar"), Some(version))
		.await
		.unwrap()
		.reduce_language_file(&OsFilesystem, &pack_file(pack_dir.path(), COMPACT_PATH))
		.await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn compact_language_keys_apply_the_changes_of_every_version() {
	let pack = br#"{"a": "one", "b": "2", "d": "4", "z": "26"}"#;

	// 1.2 has {a: one, b: two, d: 4}
	assert_eq!(
		compact_reduction("1.2", pack, false, false).await,
		LanguageFileReduction::Reduced { data: br#"{"b":"2","z":"26"}"#.to_vec() }
	);
	// 1.1 has {a: 1, b: two, d: 4}
	assert_eq!(
		compact_reduction("1.1", pack, false, false).await,
		LanguageFileReduction::Reduced { data: br#"{"a":"one","b":"2","z":"26"}"#.to_vec() }
	);
	// 1.0 has {a: 1, b: 2, c: 3}
	assert_eq!(
		compact_reduction("1.0", pack, false, false).await,
		LanguageFileReduction::Reduced { data: br#"{"a":"one","d":"4","z":"26"}"#.to_vec() }
	);
	// The whole file is the same as the 1.2 one
	assert_eq!(
		compact_reduction("1.2", br#"{"d": "4", "b": "two", "a": "one"}"#, false, false).await,
		LanguageFileReduction::Redundant
	);
	// An entry that an older version had, and a newer one changed, is not removed
	assert_eq!(
		compact_reduction("1.2", br#"{"c": "3", "b": "2"}"#, false, false).await,
		LanguageFileReduction::Unchanged
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn compact_language_keys_that_cannot_be_trusted_are_not_used() {
	let pack = br#"{"a": "one", "d": "4"}"#;

	// A change with the wrong digest breaks the versions after it, but not the ones before
	assert_eq!(compact_reduction("1.2", pack, true, false).await, LanguageFileReduction::Unchanged);
	assert_eq!(compact_reduction("1.1", pack, true, false).await, LanguageFileReduction::Unchanged);
	assert_eq!(
		compact_reduction("1.0", br#"{"a": "1"}"#, true, false).await,
		LanguageFileReduction::Redundant
	);

	// Changes without the file that they change are useless
	assert_eq!(compact_reduction("1.2", pack, false, true).await, LanguageFileReduction::Unchanged);
}
