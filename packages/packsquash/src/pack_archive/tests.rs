use super::*;
use std::io::Write;

type Files<'a> = &'a [(&'a str, &'a [u8])];

const MCMETA: &[u8] = br#"{"pack": {"pack_format": 34, "description": "test"}}"#;
const BEDROCK_MANIFEST: &[u8] = br#"{
	// Bedrock manifests may have comments
	"format_version": 2,
	"header": {"name": "test", "uuid": "00000000-0000-0000-0000-000000000000", "version": [1, 0, 0]},
	"modules": [{"type": "resources", "uuid": "00000000-0000-0000-0000-000000000001", "version": [1, 0, 0]}]
}"#;
const BEHAVIOR_MANIFEST: &[u8] =
	br#"{"format_version": 2, "header": {"name": "x"}, "modules": [{"type": "data"}]}"#;

fn java_pack(prefix: &str) -> Vec<(String, Vec<u8>)> {
	vec![
		(format!("{prefix}pack.mcmeta"), MCMETA.to_vec()),
		(
			format!("{prefix}assets/minecraft/lang/en_us.json"),
			b"{}".to_vec()
		),
		(
			format!("{prefix}assets/minecraft/textures/a.txt"),
			b"hello".to_vec()
		)
	]
}

/// A pack layout case: its name, the files of the archive and the pack roots that must be found.
type LayoutCase<'a> = (&'a str, Vec<(String, Vec<u8>)>, Vec<&'a str>);

fn as_files(files: &[(String, Vec<u8>)]) -> Vec<(&str, &[u8])> {
	files
		.iter()
		.map(|(n, d)| (n.as_str(), d.as_slice()))
		.collect()
}

fn zip_bytes(files: Files<'_>) -> Vec<u8> {
	let mut writer = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
	for (name, data) in files {
		writer
			.start_file(*name, zip::write::SimpleFileOptions::default())
			.unwrap();
		writer.write_all(data).unwrap();
	}
	writer.finish().unwrap().into_inner()
}

/// A TAR archive. The names are written as they are, even if they are not safe.
fn tar_bytes(files: Files<'_>) -> Vec<u8> {
	let mut builder = tar::Builder::new(Vec::new());
	for (name, data) in files {
		let mut header = tar::Header::new_gnu();
		header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
		header.set_size(data.len() as u64);
		header.set_mode(0o644);
		header.set_cksum();
		builder.append(&header, *data).unwrap();
	}
	builder.into_inner().unwrap()
}

fn gzip(data: &[u8]) -> Vec<u8> {
	let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
	encoder.write_all(data).unwrap();
	encoder.finish().unwrap()
}

fn xz(data: &[u8]) -> Vec<u8> {
	let mut encoder = xz2::write::XzEncoder::new(Vec::new(), 3);
	encoder.write_all(data).unwrap();
	encoder.finish().unwrap()
}

fn bzip(data: &[u8]) -> Vec<u8> {
	let mut encoder = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
	encoder.write_all(data).unwrap();
	encoder.finish().unwrap()
}

fn zstd(data: &[u8]) -> Vec<u8> {
	ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
}

fn write(directory: &Path, name: &str, data: &[u8]) -> PathBuf {
	let path = directory.join(name);
	fs::write(&path, data).unwrap();
	path
}

fn seven_zip(directory: &Path, name: &str, files: Files<'_>) -> PathBuf {
	let source = directory.join(format!("{name}-source"));
	for (file, data) in files {
		let path = source.join(file);
		fs::create_dir_all(path.parent().unwrap()).unwrap();
		fs::write(path, data).unwrap();
	}
	let archive = directory.join(name);
	sevenz_rust2::compress_to_path(&source, &archive).unwrap();
	archive
}

#[test]
fn unsafe_names_are_rejected_and_the_rest_is_cleaned() {
	for name in [
		"../evil",
		"a/../../b",
		"/etc/passwd",
		"\\absolute",
		"C:\\windows\\x",
		"a/b:stream",
		"a\0b",
		"..",
		""
	] {
		assert_eq!(safe_relative_name(name), None, "{name:?}");
	}

	assert_eq!(
		safe_relative_name("a/b/c.txt").as_deref(),
		Some("a/b/c.txt")
	);
	assert_eq!(
		safe_relative_name("a\\b\\c.txt").as_deref(),
		Some("a/b/c.txt")
	);
	assert_eq!(safe_relative_name("./a//b/./c").as_deref(), Some("a/b/c"));
	assert_eq!(safe_relative_name("dir/").as_deref(), Some("dir"));
}

#[test]
fn every_kind_of_archive_is_recognized_by_its_contents() {
	let directory = tempfile::tempdir().unwrap();
	let pack = java_pack("");
	let files = as_files(&pack);
	let tar = tar_bytes(&files);

	// The extensions are wrong on purpose: only the contents count
	let cases = [
		(
			write(directory.path(), "a.dat", &zip_bytes(&files)),
			Some(ArchiveFormat::Zip)
		),
		(
			write(directory.path(), "b.dat", &tar),
			Some(ArchiveFormat::Tar(TarCompression::None))
		),
		(
			write(directory.path(), "c.zip", &gzip(&tar)),
			Some(ArchiveFormat::Tar(TarCompression::Gzip))
		),
		(
			write(directory.path(), "d.zip", &xz(&tar)),
			Some(ArchiveFormat::Tar(TarCompression::Xz))
		),
		(
			write(directory.path(), "e.zip", &zstd(&tar)),
			Some(ArchiveFormat::Tar(TarCompression::Zstd))
		),
		(
			write(directory.path(), "f.zip", &bzip(&tar)),
			Some(ArchiveFormat::Tar(TarCompression::Bzip2))
		),
		(
			seven_zip(directory.path(), "g.zip", &files),
			Some(ArchiveFormat::SevenZip)
		),
		(
			write(directory.path(), "h.rar", b"Rar!\x1A\x07\x01\x00rest"),
			Some(ArchiveFormat::Helper)
		),
		// Not archives
		(write(directory.path(), "i.zip", b"just text"), None),
		(
			write(
				directory.path(),
				"j.gz",
				&gzip(b"one compressed file, not a tar archive")
			),
			None
		),
		(write(directory.path(), "k.zip", b""), None)
	];

	for (path, expected) in cases {
		assert_eq!(
			detect_format(&path).unwrap(),
			expected,
			"{}",
			path.display()
		);
	}

	assert_eq!(detect_format(directory.path()).unwrap(), None);
}

#[test]
fn packs_are_found_in_every_layout() {
	let directory = tempfile::tempdir().unwrap();

	let mut at_root = java_pack("");
	at_root.push(("readme.txt".into(), b"x".to_vec()));
	let in_folder = java_pack("My Pack/");
	let mut two_packs = java_pack("one/");
	two_packs.extend(java_pack("deep/er/two/"));
	let mut bedrock = vec![
		("manifest.json".to_string(), BEDROCK_MANIFEST.to_vec()),
		("textures/blocks/a.png".to_string(), b"x".to_vec())
	];
	bedrock.push(("pack_icon.png".into(), b"x".to_vec()));
	let not_a_pack = vec![
		("pack.mcmeta".to_string(), MCMETA.to_vec()), // no assets or data next to it
		("lib/mod.class".to_string(), b"x".to_vec())
	];
	let behavior_pack = vec![("manifest.json".to_string(), BEHAVIOR_MANIFEST.to_vec())];
	let too_deep = java_pack("a/b/c/d/e/f/");
	let data_pack = vec![
		("pack.mcmeta".to_string(), MCMETA.to_vec()),
		("data/minecraft/recipes/x.json".to_string(), b"{}".to_vec())
	];
	let mut inside_a_pack = java_pack("");
	inside_a_pack.extend(java_pack("nested/"));
	let mut with_junk = java_pack("__MACOSX/");
	with_junk.extend(java_pack("real/"));

	let cases: Vec<LayoutCase> = vec![
		("at_root", at_root, vec![""]),
		("in_folder", in_folder, vec!["My Pack"]),
		("two_packs", two_packs, vec!["deep/er/two", "one"]),
		("bedrock", bedrock, vec![""]),
		("not_a_pack", not_a_pack, vec![]),
		("behavior_pack", behavior_pack, vec![]),
		("too_deep", too_deep, vec![]),
		("data_pack", data_pack, vec![""]),
		("inside_a_pack", inside_a_pack, vec![""]),
		("with_junk", with_junk, vec!["real"])
	];

	for (name, pack, expected) in cases {
		let files = as_files(&pack);
		for (extension, bytes) in [
			("zip", zip_bytes(&files)),
			("tar", tar_bytes(&files)),
			("tgz", gzip(&tar_bytes(&files)))
		] {
			let archive = write(directory.path(), &format!("{name}.{extension}"), &bytes);
			let scan = scan_archive(&archive, DEFAULT_MAX_PACK_DEPTH)
				.unwrap()
				.unwrap();

			assert_eq!(
				scan.pack_roots,
				Some(expected.iter().map(ToString::to_string).collect()),
				"{name}.{extension}"
			);
		}
	}

	// A 7z archive with a pack in a folder
	let pack = java_pack("folder/");
	let archive = seven_zip(directory.path(), "pack7.7z", &as_files(&pack));
	let scan = scan_archive(&archive, DEFAULT_MAX_PACK_DEPTH)
		.unwrap()
		.unwrap();
	assert_eq!(scan.pack_roots, Some(vec!["folder".to_string()]));

	// Something that is not an archive
	let text = write(directory.path(), "text.zip", b"hello");
	assert_eq!(scan_archive(&text, DEFAULT_MAX_PACK_DEPTH).unwrap(), None);
}

#[test]
fn extracted_packs_are_complete_in_every_format() {
	let directory = tempfile::tempdir().unwrap();
	let mut files = java_pack("Some Pack/");
	files.push(("Other stuff/big.bin".to_string(), vec![7; 1000]));
	let borrowed = as_files(&files);
	let tar = tar_bytes(&borrowed);

	let archives = [
		write(directory.path(), "a", &zip_bytes(&borrowed)),
		write(directory.path(), "b", &tar),
		write(directory.path(), "c", &gzip(&tar)),
		write(directory.path(), "d", &xz(&tar)),
		write(directory.path(), "e", &zstd(&tar)),
		write(directory.path(), "f", &bzip(&tar)),
		seven_zip(directory.path(), "g", &borrowed)
	];

	for archive in archives {
		let scan = scan_archive(&archive, DEFAULT_MAX_PACK_DEPTH)
			.unwrap()
			.unwrap();
		let extracted = extract_packs(
			&archive,
			&scan,
			DEFAULT_MAX_PACK_DEPTH,
			ExtractLimits::default()
		)
		.unwrap();

		assert_eq!(extracted.packs.len(), 1, "{}", archive.display());
		let (root, path) = &extracted.packs[0];
		assert_eq!(root, "Some Pack");
		assert_eq!(fs::read(path.join("pack.mcmeta")).unwrap(), MCMETA);
		assert_eq!(
			fs::read(path.join("assets/minecraft/textures/a.txt")).unwrap(),
			b"hello"
		);
		// Only the pack is extracted
		assert!(!path.parent().unwrap().join("Other stuff").exists());

		let temporary = path.parent().unwrap().to_path_buf();
		drop(extracted);
		assert!(!temporary.exists(), "the temporary directory is deleted");
	}

	// An archive without a pack
	let archive = write(
		directory.path(),
		"nopack",
		&zip_bytes(&[("readme.txt", b"x")])
	);
	let scan = scan_archive(&archive, DEFAULT_MAX_PACK_DEPTH)
		.unwrap()
		.unwrap();
	assert_eq!(scan.pack_roots, Some(vec![]));
	assert!(matches!(
		extract_packs(
			&archive,
			&scan,
			DEFAULT_MAX_PACK_DEPTH,
			ExtractLimits::default()
		),
		Err(ArchiveError::NoPack)
	));
}

#[test]
fn extraction_does_not_leave_the_destination_or_extract_links() {
	let directory = tempfile::tempdir().unwrap();
	let destination = directory.path().join("out");

	let mut builder = tar::Builder::new(Vec::new());
	let mut add = |name: &str, kind: tar::EntryType, data: &[u8], link: Option<&str>| {
		let mut header = tar::Header::new_gnu();
		header.as_old_mut().name[..name.len()].copy_from_slice(name.as_bytes());
		if let Some(link) = link {
			header.as_old_mut().linkname[..link.len()].copy_from_slice(link.as_bytes());
		}
		header.set_entry_type(kind);
		header.set_size(data.len() as u64);
		header.set_mode(0o644);
		header.set_cksum();
		builder.append(&header, data).unwrap();
	};
	add("good/file.txt", tar::EntryType::Regular, b"good", None);
	add("../evil.txt", tar::EntryType::Regular, b"evil", None);
	add("a/../../evil2.txt", tar::EntryType::Regular, b"evil", None);
	add("/tmp/evil3.txt", tar::EntryType::Regular, b"evil", None);
	add("C:\\evil4.txt", tar::EntryType::Regular, b"evil", None);
	add("link", tar::EntryType::Symlink, b"", Some("/etc"));
	add("hard", tar::EntryType::Link, b"", Some("good/file.txt"));
	add(
		"link/inside.txt",
		tar::EntryType::Regular,
		b"through the link",
		None
	);
	let archive = write(directory.path(), "bad.tar", &builder.into_inner().unwrap());

	let summary = extract_archive(
		&archive,
		ArchiveFormat::Tar(TarCompression::None),
		&destination,
		&[],
		ExtractLimits::default()
	)
	.unwrap();

	assert_eq!(summary.skipped_unsafe, 4);
	assert_eq!(summary.skipped_special, 2);
	assert_eq!(
		fs::read(destination.join("good/file.txt")).unwrap(),
		b"good"
	);
	// A file that comes after a link is a normal file in a normal directory
	assert_eq!(
		fs::read(destination.join("link/inside.txt")).unwrap(),
		b"through the link"
	);
	assert!(!directory.path().join("evil.txt").exists());
	assert!(!directory.path().join("evil2.txt").exists());
	assert!(!Path::new("/tmp/evil3.txt").exists());
	assert!(!destination.join("link").is_symlink());
	assert!(!destination.join("hard").exists());
}

#[test]
fn zip_names_with_backslashes_and_unsafe_parts_are_handled() {
	let directory = tempfile::tempdir().unwrap();
	let archive = write(
		directory.path(),
		"w.zip",
		&zip_bytes(&[
			("pack.mcmeta", MCMETA),
			("assets\\minecraft\\a.txt", b"windows"),
			("..\\evil.txt", b"evil")
		])
	);
	let destination = directory.path().join("out");

	let summary = extract_archive(
		&archive,
		ArchiveFormat::Zip,
		&destination,
		&[],
		ExtractLimits::default()
	)
	.unwrap();

	assert_eq!(summary.skipped_unsafe, 1);
	assert_eq!(
		fs::read(destination.join("assets/minecraft/a.txt")).unwrap(),
		b"windows"
	);
	assert!(!directory.path().join("evil.txt").exists());
}

#[test]
fn limits_stop_big_archives() {
	let directory = tempfile::tempdir().unwrap();
	let archive = write(
		directory.path(),
		"big.tar",
		&tar_bytes(&[("a", &[1; 600]), ("b", &[2; 600]), ("c", &[3; 10])])
	);
	let tar = ArchiveFormat::Tar(TarCompression::None);
	let out = |name: &str| directory.path().join(name);

	assert!(matches!(
		extract_archive(
			&archive,
			tar,
			&out("o1"),
			&[],
			ExtractLimits {
				max_bytes: 1000,
				max_entries: 100
			}
		),
		Err(ArchiveError::TooBig(_))
	));
	assert!(matches!(
		extract_archive(
			&archive,
			tar,
			&out("o2"),
			&[],
			ExtractLimits {
				max_bytes: 1 << 20,
				max_entries: 2
			}
		),
		Err(ArchiveError::TooBig(_))
	));
	assert!(
		extract_archive(
			&archive,
			tar,
			&out("o3"),
			&[],
			ExtractLimits {
				max_bytes: 1210,
				max_entries: 3
			}
		)
		.is_ok()
	);
}

#[test]
fn damaged_archives_give_an_error() {
	let directory = tempfile::tempdir().unwrap();
	let pack = java_pack("");
	let mut bytes = zip_bytes(&as_files(&pack));
	bytes.truncate(bytes.len() / 2);
	let archive = write(directory.path(), "cut.zip", &bytes);

	assert!(matches!(
		scan_archive(&archive, DEFAULT_MAX_PACK_DEPTH),
		Err(ArchiveError::Format(_))
	));
}

#[test]
fn packs_are_found_in_directories() {
	let directory = tempfile::tempdir().unwrap();
	let root = directory.path();
	let make = |relative: &str, files: Files<'_>| {
		for (name, data) in files {
			let path = root.join(relative).join(name);
			fs::create_dir_all(path.parent().unwrap()).unwrap();
			fs::write(path, data).unwrap();
		}
	};

	make(
		"one",
		&[("pack.mcmeta", MCMETA), ("assets/minecraft/a.txt", b"x")]
	);
	make(
		"group/two",
		&[("pack.mcmeta", MCMETA), ("data/minecraft/a.json", b"{}")]
	);
	make("group/bedrock", &[("manifest.json", BEDROCK_MANIFEST)]);
	make("group/behavior", &[("manifest.json", BEHAVIOR_MANIFEST)]);
	make("empty_pack", &[("pack.mcmeta", MCMETA)]);
	make(
		".hidden/pack",
		&[("pack.mcmeta", MCMETA), ("assets/a.txt", b"x")]
	);
	make(
		"one/nested",
		&[("pack.mcmeta", MCMETA), ("assets/a.txt", b"x")]
	);
	make(
		"a/b/c/d/e/f/too_deep",
		&[("pack.mcmeta", MCMETA), ("assets/a.txt", b"x")]
	);

	let found: Vec<_> = find_pack_directories(root, DEFAULT_MAX_PACK_DEPTH)
		.into_iter()
		.map(|path| {
			path.strip_prefix(root)
				.unwrap()
				.to_string_lossy()
				.replace('\\', "/")
		})
		.collect();

	assert_eq!(found, ["group/bedrock", "group/two", "one"]);
	assert!(is_pack_directory(&root.join("one")));
	assert!(!is_pack_directory(root));
}

#[cfg(unix)]
#[test]
fn output_of_helper_programs_has_no_links_and_obeys_the_limits() {
	let directory = tempfile::tempdir().unwrap();
	let outside = tempfile::tempdir().unwrap();
	fs::write(outside.path().join("secret.txt"), b"secret").unwrap();

	fs::create_dir_all(directory.path().join("pack/assets")).unwrap();
	fs::write(directory.path().join("pack/pack.mcmeta"), MCMETA).unwrap();
	fs::write(directory.path().join("pack/assets/a.txt"), b"hello").unwrap();
	std::os::unix::fs::symlink(
		outside.path().join("secret.txt"),
		directory.path().join("pack/assets/link.txt")
	)
	.unwrap();
	std::os::unix::fs::symlink(outside.path(), directory.path().join("pack/dir_link")).unwrap();

	clean_helper_output(directory.path(), ExtractLimits::default()).unwrap();

	assert!(directory.path().join("pack/assets/a.txt").is_file());
	assert!(fs::symlink_metadata(directory.path().join("pack/assets/link.txt")).is_err());
	assert!(fs::symlink_metadata(directory.path().join("pack/dir_link")).is_err());
	assert!(
		outside.path().join("secret.txt").is_file(),
		"the target of a link is never touched"
	);

	let too_many_bytes = ExtractLimits {
		max_bytes: 10,
		..ExtractLimits::default()
	};
	assert!(matches!(
		clean_helper_output(directory.path(), too_many_bytes),
		Err(ArchiveError::TooBig(_))
	));
	let too_many_entries = ExtractLimits {
		max_entries: 2,
		..ExtractLimits::default()
	};
	assert!(matches!(
		clean_helper_output(directory.path(), too_many_entries),
		Err(ArchiveError::TooBig(_))
	));
}
