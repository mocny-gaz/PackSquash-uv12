use std::fs;
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::{DeflateEncoder, ZlibEncoder};
use tempfile::{Builder, TempDir};

use super::*;
use crate::config::{GlobalOptions, SquashOptions};
use crate::vfs::os_fs::OsFilesystem;
use crate::{PackSquasher, PackSquasherStatus, RelativePath};

pub(crate) fn temp_dir() -> TempDir {
	Builder::new()
		.prefix("ps-baseline-test")
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

async fn baseline_directory(directory: &TempDir) -> BaselinePack {
	BaselinePack::open(&OsFilesystem, directory.path(), None)
		.await
		.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn identical_files_are_redundant() {
	let (baseline_dir, pack_dir) = (temp_dir(), temp_dir());
	let file = "assets/minecraft/lang/en_us.lang";

	write_file(baseline_dir.path(), file, b"a=b\n");
	write_file(pack_dir.path(), file, b"a=b\n");
	write_file(
		baseline_dir.path(),
		"assets/minecraft/lang/es_es.lang",
		b"a=b\n"
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/lang/es_es.lang",
		b"a=c\n"
	);
	write_file(pack_dir.path(), "assets/minecraft/lang/new.lang", b"a=b\n");

	let baseline = baseline_directory(&baseline_dir).await;
	let is_redundant = |file: &str| {
		let pack_file = pack_file(pack_dir.path(), file);
		let baseline = &baseline;
		async move { baseline.is_redundant(&OsFilesystem, &pack_file).await }
	};

	assert!(is_redundant(file).await);
	assert!(!is_redundant("assets/minecraft/lang/es_es.lang").await);
	assert!(!is_redundant("assets/minecraft/lang/new.lang").await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn pack_metadata_is_never_redundant() {
	let (baseline_dir, pack_dir) = (temp_dir(), temp_dir());

	for file in ["pack.mcmeta", "pack.png"] {
		write_file(baseline_dir.path(), file, b"same");
		write_file(pack_dir.path(), file, b"same");
	}

	let baseline = baseline_directory(&baseline_dir).await;

	for file in ["pack.mcmeta", "pack.png"] {
		assert!(
			!baseline
				.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), file))
				.await
		);
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn json_files_are_compared_by_contents() {
	let (baseline_dir, pack_dir) = (temp_dir(), temp_dir());

	write_file(
		baseline_dir.path(),
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
		baseline_dir.path(),
		"assets/minecraft/models/item/b.json",
		br#"{"parent": "item/generated"}"#
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/models/item/b.json",
		br#"{"parent": "item/handheld"}"#
	);

	let baseline = baseline_directory(&baseline_dir).await;

	assert!(
		baseline
			.is_redundant(
				&OsFilesystem,
				&pack_file(pack_dir.path(), "assets/minecraft/models/item/a.json")
			)
			.await
	);
	assert!(
		!baseline
			.is_redundant(
				&OsFilesystem,
				&pack_file(pack_dir.path(), "assets/minecraft/models/item/b.json")
			)
			.await
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn textures_are_compared_by_pixels() {
	let (baseline_dir, pack_dir) = (temp_dir(), temp_dir());

	let baseline_pixels = [
		1, 2, 3, 255, 0, 0, 0, 0, //
		9, 8, 7, 255, 4, 5, 6, 128
	];
	// The color of the fully transparent pixel is not visible
	let mut same_pixels = baseline_pixels;
	same_pixels[4..7].copy_from_slice(&[10, 20, 30]);
	let mut other_pixels = baseline_pixels;
	other_pixels[0] = 200;

	let baseline_png = png(2, 2, &baseline_pixels, Compression::best());
	let reencoded_png = png(2, 2, &same_pixels, Compression::none());
	assert_ne!(baseline_png, reencoded_png);

	write_file(
		baseline_dir.path(),
		"assets/minecraft/textures/a.png",
		&baseline_png
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/a.png",
		&reencoded_png
	);
	write_file(
		baseline_dir.path(),
		"assets/minecraft/textures/b.png",
		&baseline_png
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/b.png",
		&png(2, 2, &other_pixels, Compression::best())
	);
	write_file(
		baseline_dir.path(),
		"assets/minecraft/textures/c.png",
		&baseline_png
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/c.png",
		&png(4, 1, &baseline_pixels, Compression::best())
	);
	write_file(
		baseline_dir.path(),
		"assets/minecraft/textures/d.png",
		&baseline_png
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/textures/d.png",
		b"not a PNG"
	);

	let baseline = baseline_directory(&baseline_dir).await;
	let is_redundant = |name: &str| {
		let pack_file = pack_file(
			pack_dir.path(),
			&format!("assets/minecraft/textures/{name}.png")
		);
		let baseline = &baseline;
		async move { baseline.is_redundant(&OsFilesystem, &pack_file).await }
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

	// (pack texture, pack metadata, baseline metadata, are the texture and metadata redundant)
	let cases: [(&[u8], Option<&[u8]>, Option<&[u8]>, bool); 6] = [
		(&texture, Some(reformatted_metadata), Some(metadata), true),
		(&texture, None, None, true),
		(&texture, Some(other_metadata), Some(metadata), false),
		(&other_texture, Some(metadata), Some(metadata), false),
		(&texture, None, Some(metadata), false),
		(&texture, Some(metadata), None, false)
	];

	for (case, (pack_texture, pack_metadata, baseline_metadata, expected)) in cases.iter().enumerate()
	{
		let (baseline_dir, pack_dir) = (temp_dir(), temp_dir());
		let texture_path = "assets/minecraft/textures/blocks/lava_flow.png";

		write_file(baseline_dir.path(), texture_path, &texture);
		write_file(pack_dir.path(), texture_path, pack_texture);

		if let Some(metadata) = baseline_metadata {
			write_file(
				baseline_dir.path(),
				&format!("{texture_path}.mcmeta"),
				metadata
			);
		}
		if let Some(metadata) = pack_metadata {
			write_file(pack_dir.path(), &format!("{texture_path}.mcmeta"), metadata);
		}

		let baseline = baseline_directory(&baseline_dir).await;

		assert_eq!(
			baseline
				.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), texture_path))
				.await,
			*expected,
			"texture of case {case}"
		);

		if pack_metadata.is_some() {
			assert_eq!(
				baseline
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
async fn archives_can_be_baseline_packs() {
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

	let baseline = BaselinePack::open(&OsFilesystem, &archive_dir.path().join("client.jar"), None)
		.await
		.unwrap();

	for (file, expected) in [
		("assets/minecraft/lang/en_us.lang", true),
		("assets/minecraft/textures/a.png", true),
		("assets/minecraft/textures/a.png.mcmeta", true),
		("assets/minecraft/textures/b.png", false)
	] {
		assert_eq!(
			baseline
				.is_redundant(&OsFilesystem, &pack_file(pack_dir.path(), file))
				.await,
			expected,
			"{file}"
		);
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn unusable_baseline_packs_are_rejected() {
	let dir = temp_dir();
	write_file(dir.path(), "not_a_zip.jar", b"definitely not a ZIP file");

	assert!(matches!(
		BaselinePack::open(&OsFilesystem, &dir.path().join("not_a_zip.jar"), None).await,
		Err(BaselinePackError::InvalidArchive(_))
	));
	assert!(matches!(
		BaselinePack::open(&OsFilesystem, &dir.path().join("missing"), None).await,
		Err(BaselinePackError::IoError(_))
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
fn squashing_leaves_out_files_identical_to_the_baseline() {
	let (baseline_dir, pack_dir, output_dir) = (temp_dir(), temp_dir(), temp_dir());

	let stone = png(2, 1, &[1, 2, 3, 255, 4, 5, 6, 255], Compression::best());
	let dirt = png(2, 1, &[7, 7, 7, 255, 8, 8, 8, 255], Compression::best());
	let modified_dirt = png(2, 1, &[7, 7, 7, 255, 9, 9, 9, 255], Compression::best());

	write_file(
		baseline_dir.path(),
		"assets/minecraft/textures/blocks/stone.png",
		&stone
	);
	write_file(
		baseline_dir.path(),
		"assets/minecraft/textures/blocks/dirt.png",
		&dirt
	);

	write_file(
		pack_dir.path(),
		"pack.mcmeta",
		br#"{"pack": {"pack_format": 1, "description": "Test pack"}}"#
	);
	// Re-encoded, but with the same pixels as the baseline
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
		baseline_pack_path: Some(baseline_dir.path().to_path_buf()),
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

	let mut skipped_due_to_baseline = Vec::new();
	while let Ok(status) = receiver.try_recv() {
		if let PackSquasherStatus::PackFileProcessed(status) = status {
			assert!(status.optimization_error().is_none());

			if status.intentionally_skipped() {
				assert!(status.skipped());
				skipped_due_to_baseline.push(status.path().as_str().to_owned());
			}
		}
	}
	assert_eq!(
		skipped_due_to_baseline,
		["assets/minecraft/textures/blocks/stone.png"]
	);

	let output = ZipArchive::parse(fs::read(output_path).unwrap()).unwrap();
	assert!(output.contains("pack.mcmeta"));
	assert!(output.contains("assets/minecraft/textures/blocks/dirt.png"));
	assert!(!output.contains("assets/minecraft/textures/blocks/stone.png"));
}

fn language_entry(root: &Path, file: &str, pack_data: &[u8], baseline_data: &[u8]) -> (TempDir, VfsPackFileIterEntry) {
	let baseline_dir = temp_dir();
	write_file(baseline_dir.path(), file, baseline_data);
	write_file(root, file, pack_data);

	(baseline_dir, pack_file(root, file))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn json_language_keys_identical_to_the_baseline_are_removed() {
	let file = "assets/minecraft/lang/es_es.json";
	let baseline = br#"{"a": "1", "b": "2", "c": "3"}"#;

	let reduce = |pack: &'static [u8]| async move {
		let pack_dir = temp_dir();
		let (baseline_dir, entry) = language_entry(pack_dir.path(), file, pack, baseline);
		baseline_directory(&baseline_dir)
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
async fn legacy_language_keys_identical_to_the_baseline_are_removed() {
	let file = "assets/minecraft/lang/en_US.lang";
	let baseline = b"a=1\nb=2\nc=3 \nd=\n";

	let reduce = |pack: &'static [u8]| async move {
		let pack_dir = temp_dir();
		let (baseline_dir, entry) = language_entry(pack_dir.path(), file, pack, baseline);
		baseline_directory(&baseline_dir)
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
	let (baseline_dir, pack_dir) = (temp_dir(), temp_dir());

	for file in [
		"assets/minecraft/texts/credits.json",
		"assets/minecraft/lang/notes.txt",
		"lang/es_es.json"
	] {
		write_file(baseline_dir.path(), file, br#"{"a": "1"}"#);
		write_file(pack_dir.path(), file, br#"{"a": "1", "b": "2"}"#);
	}

	let baseline = baseline_directory(&baseline_dir).await;

	for file in [
		"assets/minecraft/texts/credits.json",
		"assets/minecraft/lang/notes.txt",
		"lang/es_es.json"
	] {
		assert_eq!(
			baseline
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
			("baseline.json", index.as_bytes(), true),
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
async fn versioned_baseline_packs_provide_the_files_of_the_selected_version() {
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
		let baseline = BaselinePack::open(
			&OsFilesystem,
			&jar_dir.path().join("vanilla.jar"),
			Some(version)
		)
		.await
		.unwrap();

		for (name, expected) in ["a", "b", "c"].into_iter().zip(expected) {
			assert_eq!(
				baseline
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
async fn versioned_baseline_packs_can_be_directories() {
	let baseline_dir = temp_dir();
	write_file(baseline_dir.path(), "baseline.json", VERSION_INDEX.as_bytes());
	write_file(baseline_dir.path(), "1.0/assets/minecraft/texts/a.txt", b"a from 1.0");
	write_file(baseline_dir.path(), "1.1/assets/minecraft/texts/a.txt", b"a from 1.1");

	let pack_dir = temp_dir();
	write_file(pack_dir.path(), "assets/minecraft/texts/a.txt", b"a from 1.1");

	let baseline = BaselinePack::open(&OsFilesystem, baseline_dir.path(), Some("1.2"))
		.await
		.unwrap();

	assert!(
		baseline
			.is_redundant(
				&OsFilesystem,
				&pack_file(pack_dir.path(), "assets/minecraft/texts/a.txt")
			)
			.await
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn versioned_baseline_packs_reject_wrong_version_selections() {
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
		async move { BaselinePack::open(&OsFilesystem, &path, version).await.err() }
	};

	assert!(matches!(
		open("vanilla.jar", None).await,
		Some(BaselinePackError::VersionRequired(versions)) if versions == "1.0, 1.1, 1.2"
	));
	assert!(matches!(
		open("vanilla.jar", Some("9.9")).await,
		Some(BaselinePackError::UnknownVersion { version, .. }) if version == "9.9"
	));
	assert!(matches!(
		open("plain.jar", Some("1.0")).await,
		Some(BaselinePackError::NotVersioned)
	));
	assert!(matches!(
		open("bad_format.jar", Some("1.0")).await,
		Some(BaselinePackError::InvalidVersionIndex(_))
	));
	assert!(matches!(
		open("bad_index.jar", Some("1.0")).await,
		Some(BaselinePackError::InvalidVersionIndex(_))
	));
	assert!(open("plain.jar", None).await.is_none());
}

/// Not async because `PackSquasher::run` starts its own runtime.
#[test]
fn squashing_removes_language_keys_identical_to_the_baseline() {
	for skip_language_keys in [true, false] {
		let (baseline_dir, pack_dir, output_dir) = (temp_dir(), temp_dir(), temp_dir());

		write_file(baseline_dir.path(), "assets/minecraft/lang/en_US.lang", b"a=1\nb=2\nc=3\n");
		write_file(baseline_dir.path(), "assets/minecraft/lang/fr_FR.lang", b"a=1\nb=2\n");

		write_file(
			pack_dir.path(),
			"pack.mcmeta",
			br#"{"pack": {"pack_format": 1, "description": "Test pack"}}"#
		);
		write_file(pack_dir.path(), "assets/minecraft/lang/en_US.lang", b"a=1\nb=9\nc=3\nd=4\n");
		write_file(pack_dir.path(), "assets/minecraft/lang/fr_FR.lang", b"a=1\n");

		let output_path = output_dir.path().join("pack.zip");
		let mut global_options = GlobalOptions {
			baseline_pack_path: Some(baseline_dir.path().to_path_buf()),
			skip_language_keys_identical_to_baseline: skip_language_keys,
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
