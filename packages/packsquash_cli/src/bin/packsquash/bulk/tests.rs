use std::{cell::RefCell, fs, path::Path};

use tempfile::TempDir;

use packsquash::pack_archive::{ArchiveFormat, NestedArchive};

use super::*;

fn options(root: &Path) -> BulkOptions {
	BulkOptions {
		input_directory: root.to_path_buf(),
		output_directory: None,
		output_prefix: Some("squashed_".to_string()),
		output_suffix: String::new(),
		overwrite: false,
		backup: true,
		dry_run: false,
		report_file: None,
		archive_extensions: ["zip", "7z", "tar", "tgz"].map(String::from).to_vec(),
		scan_depth: 32
	}
}

/// Writes a pack in a directory. The description of the pack is its name.
fn write_pack(directory: &Path, name: &str) {
	fs::create_dir_all(directory.join("assets/minecraft")).unwrap();
	fs::write(
		directory.join("pack.mcmeta"),
		format!("{{\"pack\":{{\"pack_format\":34,\"description\":\"{name}\"}}}}")
	)
	.unwrap();
	fs::write(directory.join("assets/minecraft/a.txt"), b"hello").unwrap();
}

/// Writes a TAR file that has the given packs, each in the given directory of the archive.
fn write_tar(path: &Path, packs: &[&str]) {
	let mut builder = tar::Builder::new(fs::File::create(path).unwrap());

	for pack in packs {
		let prefix = if pack.is_empty() {
			String::new()
		} else {
			format!("{pack}/")
		};
		let mcmeta = format!("{{\"pack\":{{\"pack_format\":34,\"description\":\"{pack}\"}}}}");

		for (name, data) in [
			(format!("{prefix}pack.mcmeta"), mcmeta.as_bytes()),
			(format!("{prefix}assets/minecraft/a.txt"), &b"hello"[..])
		] {
			let mut header = tar::Header::new_gnu();
			header.set_size(data.len() as u64);
			header.set_mode(0o644);
			header.set_cksum();
			builder.append_data(&mut header, name, data).unwrap();
		}
	}

	builder.finish().unwrap();
}

fn write_tar_without_pack(path: &Path) {
	let mut builder = tar::Builder::new(fs::File::create(path).unwrap());
	let mut header = tar::Header::new_gnu();
	header.set_size(2);
	header.set_mode(0o644);
	header.set_cksum();
	builder
		.append_data(&mut header, "readme.txt", &b"hi"[..])
		.unwrap();
	builder.finish().unwrap();
}

/// Writes a TAR file with the given files, which can be archives made with `tar_bytes`.
fn write_tar_files(path: &Path, files: &[(&str, Vec<u8>)]) {
	fs::write(path, tar_files_bytes(files)).unwrap();
}

fn tar_files_bytes(files: &[(&str, Vec<u8>)]) -> Vec<u8> {
	let mut builder = tar::Builder::new(Vec::new());

	for (name, data) in files {
		let mut header = tar::Header::new_gnu();
		header.set_size(data.len() as u64);
		header.set_mode(0o644);
		header.set_cksum();
		builder.append_data(&mut header, name, &data[..]).unwrap();
	}

	builder.into_inner().unwrap()
}

/// The bytes of a TAR archive that has the given packs, each in the given directory.
fn tar_bytes(packs: &[&str]) -> Vec<u8> {
	let mut files = Vec::new();

	for pack in packs {
		let prefix = if pack.is_empty() {
			String::new()
		} else {
			format!("{pack}/")
		};

		files.push((
			format!("{prefix}pack.mcmeta"),
			format!("{{\"pack\":{{\"pack_format\":34,\"description\":\"{pack}\"}}}}").into_bytes()
		));
		files.push((format!("{prefix}assets/minecraft/a.txt"), b"hello".to_vec()));
	}

	let files: Vec<(&str, Vec<u8>)> = files
		.iter()
		.map(|(name, data)| (name.as_str(), data.clone()))
		.collect();

	tar_files_bytes(&files)
}

/// Searches for packs with the layout of the options.
fn find(options: &BulkOptions, depth: usize) -> Vec<Found> {
	discover(options, &Layout::new(options), depth)
}

struct Run {
	code: i32,
	/// The result files that the squash function was asked to write.
	outputs: Vec<PathBuf>
}

/// Runs the bulk mode with a squash function that writes a small result file. A pack with `bad` in the description
/// fails. The function checks that the pack exists when it is called, because the files of an archive are deleted after.
fn run_bulk(options: &BulkOptions) -> Run {
	run_bulk_with_depth(options, 4)
}

fn run_bulk_with_depth(options: &BulkOptions, depth: usize) -> Run {
	let outputs = RefCell::new(Vec::new());
	let table: Table = "".parse().unwrap();

	let mut squash = |squash_options: SquashOptions| -> Result<(), String> {
		let pack = squash_options.pack_directory.clone();
		let output = squash_options.global_options.output_file_path.clone();
		let mcmeta = fs::read_to_string(pack.join("pack.mcmeta"))
			.map_err(|error| format!("the pack was not there: {error}"))?;

		outputs.borrow_mut().push(output.clone());

		if mcmeta.contains("bad") {
			return Err("this pack is bad".to_string());
		}

		fs::write(output, b"RESULT").map_err(|error| error.to_string())
	};

	let code = run(
		options,
		depth,
		ExtractLimits::default(),
		&table,
		&mut squash
	);

	Run {
		code,
		outputs: outputs.into_inner()
	}
}

fn names(paths: &[PathBuf]) -> Vec<String> {
	paths.iter().map(|path| file_name_of(path)).collect()
}

#[test]
fn archive_stems_lose_the_archive_extensions() {
	for (name, stem) in [
		("pack.zip", "pack"),
		("pack.tar.gz", "pack"),
		("Pack.TAR.XZ", "Pack"),
		("pack.tgz", "pack"),
		("a.b.7z", "a.b"),
		("my.pack.tar", "my.pack"),
		("noextension", "noextension"),
		(".zip", ".zip")
	] {
		assert_eq!(archive_stem(name), stem, "{name}");
	}
}

#[test]
fn result_names_use_the_prefix_and_the_suffix() {
	let mut bulk = options(Path::new("in"));
	let layout = Layout::new(&bulk);

	assert_eq!(layout.result_name("Pack"), "squashed_Pack");
	assert_eq!(
		layout.result_directory(Path::new("in/dir")),
		Path::new("in/dir"),
		"the results are next to the input"
	);

	bulk.output_prefix = Some(String::new());
	bulk.output_suffix = "_small".to_string();

	assert_eq!(Layout::new(&bulk).result_name("Pack"), "Pack_small");
}

#[test]
fn an_output_directory_removes_the_default_prefix_and_keeps_the_folders() {
	let mut bulk = options(Path::new("in"));
	bulk.output_directory = Some(PathBuf::from("out"));
	bulk.output_prefix = None;

	let layout = Layout::new(&bulk);

	assert_eq!(layout.result_name("Pack"), "Pack", "no prefix by default");
	assert_eq!(
		layout.result_directory(Path::new("in/a/b")),
		Path::new("out/a/b")
	);
	assert_eq!(layout.result_directory(Path::new("in")), Path::new("out"));

	bulk.output_prefix = Some("x_".to_string());

	assert_eq!(Layout::new(&bulk).result_name("Pack"), "x_Pack");

	// The input directory as the output directory is the same as no output directory
	bulk.output_directory = Some(PathBuf::from("in"));
	bulk.output_prefix = None;

	let layout = Layout::new(&bulk);

	assert!(layout.output_root.is_none());
	assert_eq!(layout.result_name("Pack"), "squashed_Pack");
}

#[test]
fn results_of_archives_are_a_file_or_a_folder_that_mirrors_the_inside() {
	let dest = Dest {
		directory: PathBuf::from("out/dir"),
		name: "big".to_string()
	};

	assert_eq!(dest.file(), Path::new("out/dir/big.zip"));
	assert_eq!(dest.pack_file(""), Path::new("out/dir/big/big.zip"));
	assert_eq!(dest.pack_file("Pack"), Path::new("out/dir/big/Pack.zip"));
	assert_eq!(
		dest.pack_file("a/b/Pack"),
		Path::new("out/dir/big/a/b/Pack.zip")
	);

	let nested = dest.nested("a/inner.tar.gz");

	assert_eq!(nested.directory, Path::new("out/dir/big/a"));
	assert_eq!(nested.name, "inner");
	assert_eq!(dest.nested("root.zip").directory, Path::new("out/dir/big"));
}

#[test]
fn nested_archives_are_extracted_in_groups_for_zip_only() {
	let nested: Vec<NestedArchive> = (0..5)
		.map(|number| NestedArchive {
			name: format!("{number}.zip"),
			size: 1024 * 1024 * 1024
		})
		.collect();

	let groups = nested_batches(&nested, ArchiveFormat::Zip);

	assert_eq!(
		groups.iter().map(Vec::len).collect::<Vec<_>>(),
		[2, 2, 1],
		"at most 2 GiB together"
	);
	assert_eq!(
		nested_batches(&nested, ArchiveFormat::SevenZip).len(),
		1,
		"a 7z archive is read from the start for every extraction, so everything is extracted at once"
	);
	assert!(nested_batches(&[], ArchiveFormat::Zip).is_empty());
}

#[test]
fn earlier_results_are_recognized() {
	let mut bulk = options(Path::new("in"));

	assert!(Layout::new(&bulk).looks_like_output("squashed_a"));
	assert!(!Layout::new(&bulk).looks_like_output("a"));

	bulk.output_prefix = Some(String::new());
	bulk.output_suffix = "_small".to_string();

	assert!(Layout::new(&bulk).looks_like_output("a_small"));
	assert!(!Layout::new(&bulk).looks_like_output("a"));

	bulk.output_suffix = String::new();

	assert!(
		!Layout::new(&bulk).looks_like_output("anything"),
		"without prefix and suffix, nothing is a result"
	);
}

#[test]
fn packs_are_found_in_directories_and_archives() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_pack(&base.join("PackA"), "a");
	// Inside a pack, nothing is searched
	write_pack(&base.join("PackA/nested/PackB"), "b");
	write_pack(&base.join("group/PackC"), "c");
	write_tar(&base.join("archive.tar"), &["Inner"]);
	write_tar_without_pack(&base.join("nopack.tar"));
	fs::write(base.join("fake.zip"), b"this is not an archive").unwrap();
	fs::write(base.join("notes.txt"), b"hi").unwrap();
	write_tar(&base.join("squashed_old.tar"), &["Old"]);

	let found = find(&options(base), 4);
	let mut kinds: Vec<String> = found
		.iter()
		.map(|item| match item {
			Found::Directory(path) => format!("dir {}", file_name_of(path)),
			Found::Archive(path, _) => format!("archive {}", file_name_of(path)),
			Found::Skipped(path, reason) => format!("skipped {} ({reason})", file_name_of(path))
		})
		.collect();
	kinds.sort();

	assert_eq!(
		kinds,
		[
			"archive archive.tar",
			"dir PackA",
			"dir PackC",
			"skipped fake.zip (it is not an archive)",
			"skipped nopack.tar (it has no pack)",
			"skipped squashed_old.tar (it looks like the result of an earlier run)"
		]
	);
}

#[test]
fn the_scan_depth_limits_the_search() {
	let root = TempDir::new().unwrap();

	// The pack directory is 4 levels below the input directory, and so is the archive
	write_pack(&root.path().join("a/b/c/Pack"), "deep");
	write_tar(&root.path().join("a/b/c/d.tar"), &["Inner"]);

	let mut bulk = options(root.path());
	bulk.scan_depth = 3;

	assert!(find(&bulk, 4).is_empty());

	bulk.scan_depth = 4;

	assert_eq!(find(&bulk, 4).len(), 2);

	// Directly in the input directory, a pack or an archive is at level 1
	write_tar(&root.path().join("top.tar"), &["Inner"]);
	bulk.scan_depth = 1;

	assert_eq!(find(&bulk, 4).len(), 1);
}

#[test]
fn results_are_written_next_to_the_inputs() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_pack(&base.join("PackA"), "a");
	write_pack(&base.join("group/PackC"), "c");
	write_tar(&base.join("archive.tar"), &["Inner"]);

	let run = run_bulk(&options(base));

	assert_eq!(run.code, 0);
	assert!(base.join("squashed_PackA.zip").is_file());
	assert!(base.join("group/squashed_PackC.zip").is_file());
	assert!(base.join("squashed_archive.zip").is_file());
	assert!(
		base.join("PackA/pack.mcmeta").is_file(),
		"directory packs are never changed"
	);
	assert!(
		base.join("archive.tar").is_file(),
		"archives are kept without bulk_overwrite"
	);
	assert_eq!(run.outputs.len(), 3);

	// A second run does not squash the results of the first one
	let second = run_bulk(&options(base));

	assert_eq!(second.code, 0);
	assert_eq!(
		second.outputs.len(),
		3,
		"only the three packs, not their results"
	);
}

#[test]
fn a_dry_run_changes_nothing() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_pack(&base.join("PackA"), "a");
	write_tar(&base.join("archive.tar"), &["Inner"]);

	let mut bulk = options(base);
	bulk.dry_run = true;
	bulk.overwrite = true;
	bulk.report_file = Some(base.join("report.txt"));

	let run = run_bulk(&bulk);
	let report = fs::read_to_string(base.join("report.txt")).unwrap();

	assert_eq!(run.code, 0);
	assert!(run.outputs.is_empty(), "no pack is squashed");
	assert!(!base.join("squashed_PackA.zip").exists());
	assert!(!base.join("archive.zip").exists());
	assert!(base.join("archive.tar").is_file());
	assert!(
		report.contains("squashed_PackA.zip") && report.contains("would be kept as"),
		"{report}"
	);
	assert!(report.contains("2 would be squashed"), "{report}");
}

#[test]
fn an_archive_can_be_replaced_and_its_original_kept() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	// The name says ZIP, but the contents are a TAR: the contents decide
	write_tar(&base.join("p.zip"), &["Inner"]);
	let original = fs::read(base.join("p.zip")).unwrap();
	write_tar(&base.join("q.tar"), &["Inner"]);

	let mut bulk = options(base);
	bulk.overwrite = true;

	let run = run_bulk(&bulk);

	assert_eq!(run.code, 0, "{:?}", run.outputs);
	assert_eq!(fs::read(base.join("p.zip")).unwrap(), b"RESULT");
	assert_eq!(fs::read(base.join("p.zip.original")).unwrap(), original);
	assert_eq!(fs::read(base.join("q.zip")).unwrap(), b"RESULT");
	assert!(base.join("q.tar.original").is_file());
	assert!(!base.join("q.tar").exists());
	assert!(
		names(&run.outputs)
			.iter()
			.all(|name| name.ends_with(".zip.squashing")),
		"the result is written to a temporary file first: {:?}",
		run.outputs
	);
	assert!(
		fs::read_dir(base)
			.unwrap()
			.flatten()
			.all(|entry| !file_name_of(&entry.path()).ends_with(".squashing"))
	);
}

#[test]
fn an_archive_can_be_replaced_without_a_backup() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_tar(&base.join("q.tar"), &["Inner"]);
	write_tar(&base.join("p.zip"), &["Inner"]);

	let mut bulk = options(base);
	bulk.overwrite = true;
	bulk.backup = false;

	assert_eq!(run_bulk(&bulk).code, 0);
	assert_eq!(fs::read(base.join("q.zip")).unwrap(), b"RESULT");
	assert_eq!(fs::read(base.join("p.zip")).unwrap(), b"RESULT");
	assert!(!base.join("q.tar").exists());
	assert!(!base.join("q.tar.original").exists() && !base.join("p.zip.original").exists());
}

#[test]
fn a_directory_is_never_replaced() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_pack(&base.join("PackA"), "a");

	let mut bulk = options(base);
	bulk.overwrite = true;

	assert_eq!(run_bulk(&bulk).code, 0);
	assert!(base.join("PackA/pack.mcmeta").is_file());
	assert!(base.join("squashed_PackA.zip").is_file());
}

#[test]
fn an_archive_with_several_packs_gives_one_result_per_pack_and_is_kept() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_tar(&base.join("two.tar"), &["a", "sub/b"]);

	let mut bulk = options(base);
	bulk.overwrite = true;
	bulk.backup = false;

	let run = run_bulk(&bulk);

	assert_eq!(run.code, 0);
	assert!(base.join("squashed_two/a.zip").is_file());
	assert!(base.join("squashed_two/sub/b.zip").is_file());
	assert!(
		base.join("two.tar").is_file(),
		"it has several packs, so it is not replaced"
	);
}

#[test]
fn a_pack_that_fails_does_not_stop_the_others() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_pack(&base.join("good"), "good");
	write_pack(&base.join("worse"), "bad");
	write_tar(&base.join("zzz.tar"), &["Inner"]);

	let mut bulk = options(base);
	bulk.report_file = Some(base.join("report.txt"));

	let run = run_bulk(&bulk);
	let report = fs::read_to_string(base.join("report.txt")).unwrap();

	assert_eq!(run.code, 1);
	assert!(base.join("squashed_good.zip").is_file());
	assert!(base.join("squashed_zzz.zip").is_file());
	assert!(report.contains("failed: this pack is bad"), "{report}");
	assert!(report.contains("2 squashed, 1 failed"), "{report}");
}

#[test]
fn a_failed_replacement_leaves_the_original_alone() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_tar(&base.join("p.tar"), &["bad"]);
	let original = fs::read(base.join("p.tar")).unwrap();

	let mut bulk = options(base);
	bulk.overwrite = true;

	assert_eq!(run_bulk(&bulk).code, 1);
	assert_eq!(fs::read(base.join("p.tar")).unwrap(), original);
	assert!(!base.join("p.zip").exists() && !base.join("p.zip.squashing").exists());
	assert!(!base.join("p.tar.original").exists());
}

#[test]
fn nothing_that_is_not_a_result_is_destroyed() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	// 1. Without prefix and suffix, the result of `p.zip` would be `p.zip` itself
	write_tar(&base.join("p.zip"), &["Inner"]);
	let original = fs::read(base.join("p.zip")).unwrap();
	let mut bulk = options(base);
	bulk.output_prefix = Some(String::new());

	assert_eq!(run_bulk(&bulk).code, 1);
	assert_eq!(fs::read(base.join("p.zip")).unwrap(), original);

	// 2. A file that is in the way of a replacement
	fs::remove_file(base.join("p.zip")).unwrap();
	write_tar(&base.join("r.7z"), &["Inner"]);
	fs::write(base.join("r.zip"), b"another file").unwrap();
	let mut bulk = options(base);
	bulk.overwrite = true;

	assert_eq!(run_bulk(&bulk).code, 1);
	assert_eq!(fs::read(base.join("r.zip")).unwrap(), b"another file");
	assert!(base.join("r.7z").is_file());

	// 3. A backup that is already there
	fs::remove_file(base.join("r.zip")).unwrap();
	fs::write(base.join("r.7z.original"), b"older backup").unwrap();

	assert_eq!(run_bulk(&bulk).code, 1);
	assert_eq!(
		fs::read(base.join("r.7z.original")).unwrap(),
		b"older backup"
	);
	assert!(base.join("r.7z").is_file());
}

#[test]
fn two_packs_with_the_same_result_name_are_not_mixed() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_pack(&base.join("X"), "dir");
	write_tar(&base.join("X.tar"), &["Inner"]);

	let run = run_bulk(&options(base));

	assert_eq!(run.code, 1, "one of them can not have the result");
	assert_eq!(
		run.outputs.len(),
		1,
		"the second pack is not squashed over the first result"
	);
	assert_eq!(fs::read(base.join("squashed_X.zip")).unwrap(), b"RESULT");
}

#[test]
fn a_missing_input_directory_is_an_error() {
	let root = TempDir::new().unwrap();
	let bulk = options(&root.path().join("does_not_exist"));

	assert_eq!(run_bulk(&bulk).code, 4);
}

#[test]
fn an_empty_directory_is_not_an_error() {
	let root = TempDir::new().unwrap();

	assert_eq!(run_bulk(&options(root.path())).code, 0);
}

#[test]
fn the_input_can_be_an_archive_file() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	// The name and the extension do not matter, the contents do
	write_tar(&base.join("two.7z"), &["a", "sub/b"]);
	write_tar(&base.join("single.rar"), &["Inner"]);

	let run = run_bulk(&options(&base.join("two.7z")));

	assert_eq!(run.code, 0);
	assert!(base.join("squashed_two/a.zip").is_file());
	assert!(base.join("squashed_two/sub/b.zip").is_file());
	assert!(
		base.join("two.7z").is_file(),
		"the archive is kept without bulk_overwrite"
	);
	assert_eq!(run.outputs.len(), 2);

	let mut bulk = options(&base.join("single.rar"));
	bulk.overwrite = true;
	bulk.backup = false;

	assert_eq!(run_bulk(&bulk).code, 0);
	assert_eq!(fs::read(base.join("single.zip")).unwrap(), b"RESULT");
	assert!(!base.join("single.rar").exists());
}

#[test]
fn an_input_file_that_is_not_an_archive_is_an_error() {
	let root = TempDir::new().unwrap();
	fs::write(root.path().join("notes.txt"), b"not an archive at all").unwrap();

	assert_eq!(run_bulk(&options(&root.path().join("notes.txt"))).code, 4);

	// An archive without a pack is like an empty directory
	write_tar_without_pack(&root.path().join("nopack.tar"));

	assert_eq!(run_bulk(&options(&root.path().join("nopack.tar"))).code, 0);
}

#[test]
fn packs_deep_inside_an_archive_are_found_with_a_deeper_search() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	// 7 directories deep: the default depth inside archives is 4
	write_tar(&base.join("deep.tar"), &["a/b/c/d/e/f/g"]);

	let outputs = RefCell::new(Vec::new());
	let table: Table = "".parse().unwrap();
	let mut squash = |squash_options: SquashOptions| -> Result<(), String> {
		outputs
			.borrow_mut()
			.push(squash_options.global_options.output_file_path.clone());
		fs::write(squash_options.global_options.output_file_path, b"RESULT")
			.map_err(|error| error.to_string())
	};

	// The default depth does not find it
	assert_eq!(
		run(
			&options(base),
			4,
			ExtractLimits::default(),
			&table,
			&mut squash
		),
		0
	);
	assert!(outputs.borrow().is_empty());

	// The depth of the bulk mode does
	assert_eq!(
		run(
			&options(base),
			10,
			ExtractLimits::default(),
			&table,
			&mut squash
		),
		0
	);
	assert_eq!(outputs.borrow().len(), 1);
}

const MCMETA: &[u8] = br#"{"pack":{"pack_format":34,"description":"x"}}"#;

/// The files of a pack, to put in a TAR file, in the given directory of it.
fn pack_files(directory: &str) -> Vec<(String, Vec<u8>)> {
	let prefix = if directory.is_empty() {
		String::new()
	} else {
		format!("{directory}/")
	};

	vec![
		(format!("{prefix}pack.mcmeta"), MCMETA.to_vec()),
		(format!("{prefix}assets/minecraft/a.txt"), b"hello".to_vec()),
	]
}

fn write_tar_entries(path: &Path, entries: Vec<(String, Vec<u8>)>) {
	let files: Vec<(&str, Vec<u8>)> = entries
		.iter()
		.map(|(name, data)| (name.as_str(), data.clone()))
		.collect();

	write_tar_files(path, &files);
}

/// A TAR file with a pack in a directory, a pack in an archive inside it, two packs in another archive inside
/// it, and an archive that has no pack.
fn write_big_tar(path: &Path) {
	let mut entries = pack_files("Loose");

	entries.push(("sub/inner.tar".to_string(), tar_bytes(&[""])));
	entries.push(("other/two.tar".to_string(), tar_bytes(&["a", "b"])));
	entries.push((
		"notes.tar".to_string(),
		tar_files_bytes(&[("readme.txt", b"hi".to_vec())])
	));

	write_tar_entries(path, entries);
}

#[test]
fn archives_inside_archives_are_opened_and_the_results_mirror_the_inside() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_big_tar(&base.join("big.tar"));

	let mut bulk = options(base);
	bulk.report_file = Some(base.join("report.txt"));

	let run = run_bulk(&bulk);

	assert_eq!(run.code, 0, "{:?}", run.outputs);

	for result in [
		"squashed_big/Loose.zip",
		"squashed_big/sub/inner.zip",
		"squashed_big/other/two/a.zip",
		"squashed_big/other/two/b.zip"
	] {
		assert!(base.join(result).is_file(), "{result}");
	}

	assert_eq!(run.outputs.len(), 4);
	assert!(base.join("big.tar").is_file(), "the input is kept");
	assert!(
		!base.join("squashed_big/notes.zip").exists() && !base.join("squashed_notes.zip").exists(),
		"an archive without a pack gives nothing"
	);

	let report = fs::read_to_string(base.join("report.txt")).unwrap();

	assert!(report.contains("skipped"), "{report}");
	assert!(report.contains("notes.tar: it has no pack"), "{report}");
	assert!(
		report.contains("4 squashed, 0 failed, 1 skipped"),
		"{report}"
	);
}

#[test]
fn labels_show_the_name_and_where_the_pack_is() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_tar(&base.join("two.tar"), &["a", "sub/b"]);
	write_big_tar(&base.join("big.tar"));

	let mut bulk = options(base);
	bulk.report_file = Some(base.join("report.txt"));

	assert_eq!(run_bulk(&bulk).code, 0);

	let report = fs::read_to_string(base.join("report.txt")).unwrap();
	let two = base.join("two.tar").display().to_string();

	// The archive with nested archives makes the total unknown, so the numbers have no total
	assert!(report.contains(&format!("] a ({two} > a)")), "{report}");
	assert!(report.contains(&format!("] b ({two} > sub/b)")), "{report}");
	assert!(
		report.contains("> sub/inner.tar > ") || report.contains("inner ("),
		"{report}"
	);

	// Without archives inside archives, the total is known
	let other = TempDir::new().unwrap();

	write_tar(&other.path().join("two.tar"), &["a", "sub/b"]);

	let mut bulk = options(other.path());
	bulk.report_file = Some(other.path().join("report.txt"));

	assert_eq!(run_bulk(&bulk).code, 0);

	let report = fs::read_to_string(other.path().join("report.txt")).unwrap();

	assert!(report.contains("[1/2] a ("), "{report}");
	assert!(report.contains("[2/2] b ("), "{report}");
}

#[test]
fn the_size_of_a_pack_is_the_size_of_the_pack_and_not_of_the_archive() {
	let root = TempDir::new().unwrap();
	let base = root.path();
	let mib = vec![7_u8; 1024 * 1024];
	let mut entries = Vec::new();

	for name in ["a", "b"] {
		entries.extend(pack_files(name));
		entries.push((format!("{name}/assets/minecraft/big.bin"), mib.clone()));
	}

	write_tar_entries(&base.join("two.tar"), entries);

	let mut bulk = options(base);
	bulk.report_file = Some(base.join("report.txt"));

	assert_eq!(run_bulk(&bulk).code, 0);

	let report = fs::read_to_string(base.join("report.txt")).unwrap();

	assert_eq!(
		report.matches("(1.00 MiB -> 0.00 MiB)").count(),
		2,
		"{report}"
	);
	assert!(report.contains("Size: 2.00 MiB -> 0.00 MiB."), "{report}");
}

#[test]
fn the_size_of_a_pack_that_is_a_whole_archive_is_the_size_of_the_archive_file() {
	let root = TempDir::new().unwrap();
	let base = root.path();
	let mut entries = pack_files("");

	entries.push((
		"assets/minecraft/big.bin".to_string(),
		vec![7_u8; 3 * 1024 * 1024]
	));

	write_tar_entries(&base.join("single.tar"), entries);

	let mut bulk = options(base);
	bulk.report_file = Some(base.join("report.txt"));

	assert_eq!(run_bulk(&bulk).code, 0);

	let size = mib(fs::metadata(base.join("single.tar")).unwrap().len());
	let report = fs::read_to_string(base.join("report.txt")).unwrap();

	assert!(report.contains(&format!("({size} -> ")), "{report}");
	assert_eq!(
		archive_pack_size(&base.join("single.tar"), "Pack"),
		None,
		"a pack in a directory of the archive counts its files"
	);
}

#[test]
fn an_output_directory_receives_the_results_without_a_prefix() {
	let input = TempDir::new().unwrap();
	let output = TempDir::new().unwrap();

	write_pack(&input.path().join("PackA"), "a");
	write_pack(&input.path().join("group/PackC"), "c");
	write_tar(&input.path().join("group/x.tar"), &["Inner"]);
	write_tar(&input.path().join("two.tar"), &["a", "sub/b"]);

	let mut bulk = options(input.path());
	bulk.output_directory = Some(output.path().to_path_buf());
	bulk.output_prefix = None;
	bulk.overwrite = true;

	let run = run_bulk(&bulk);

	assert_eq!(run.code, 0);

	for result in [
		"PackA.zip",
		"group/PackC.zip",
		"group/x.zip",
		"two/a.zip",
		"two/sub/b.zip"
	] {
		assert!(output.path().join(result).is_file(), "{result}");
	}

	assert!(
		input.path().join("group/x.tar").is_file() && input.path().join("two.tar").is_file(),
		"nothing is replaced when the results go to another directory"
	);
	assert!(
		fs::read_dir(input.path())
			.unwrap()
			.flatten()
			.all(|entry| !file_name_of(&entry.path()).ends_with(".zip")),
		"nothing is written next to the input"
	);

	// The prefix is used when the owner sets it
	let prefixed = TempDir::new().unwrap();
	let mut bulk = options(input.path());
	bulk.output_directory = Some(prefixed.path().to_path_buf());
	bulk.output_prefix = Some("small_".to_string());

	assert_eq!(run_bulk(&bulk).code, 0);
	assert!(prefixed.path().join("small_PackA.zip").is_file());
	assert!(prefixed.path().join("group/small_PackC.zip").is_file());
	assert!(prefixed.path().join("small_two/a.zip").is_file());
}

#[test]
fn an_output_directory_inside_the_input_is_not_searched() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_pack(&base.join("PackA"), "a");

	let mut bulk = options(base);
	bulk.output_directory = Some(base.join("out"));
	bulk.output_prefix = None;

	assert_eq!(run_bulk(&bulk).code, 0);
	assert!(base.join("out/PackA.zip").is_file());

	// The results are not squashed again, and the directory is not an input
	fs::create_dir_all(base.join("out/Fake")).unwrap();
	write_pack(&base.join("out/Fake"), "fake");

	let run = run_bulk(&bulk);

	assert_eq!(run.code, 0);
	assert_eq!(run.outputs.len(), 1, "{:?}", run.outputs);
}

#[test]
fn a_dry_run_does_not_open_the_archives_inside_archives() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	write_big_tar(&base.join("big.tar"));

	let mut bulk = options(base);
	bulk.dry_run = true;
	bulk.report_file = Some(base.join("report.txt"));

	let run = run_bulk(&bulk);

	assert_eq!(run.code, 0);
	assert!(run.outputs.is_empty());
	assert!(!base.join("squashed_big").exists());

	let report = fs::read_to_string(base.join("report.txt")).unwrap();

	assert!(
		report.contains("sub/inner.tar: an archive inside the archive"),
		"{report}"
	);
	assert!(
		report.contains("other/two.tar: an archive inside the archive"),
		"{report}"
	);
}

/// A TAR archive that has an archive that has an archive, and so on, `levels` times. The innermost archive has
/// a pack.
fn archives_in_archives(levels: usize) -> Vec<u8> {
	let mut archive = tar_bytes(&["Pack"]);

	for level in (1..=levels).rev() {
		archive = tar_files_bytes(&[(format!("a{level}.tar").as_str(), archive)]);
	}

	archive
}

#[test]
fn how_many_archives_can_be_inside_each_other_follows_the_search_depth() {
	// top > a1 > a2, and the pack is in a2
	let root = TempDir::new().unwrap();
	let base = root.path();

	fs::write(base.join("top.tar"), archives_in_archives(2)).unwrap();

	let run = run_bulk_with_depth(&options(base), 2);

	assert_eq!(run.code, 0);
	assert!(
		base.join("squashed_top/a1/a2.zip").is_file(),
		"{:?}",
		run.outputs
	);

	// top > a1 > a2 > a3, and the pack is in a3: one too deep
	let root = TempDir::new().unwrap();
	let base = root.path();

	fs::write(base.join("top.tar"), archives_in_archives(3)).unwrap();

	let mut bulk = options(base);
	bulk.report_file = Some(base.join("report.txt"));

	let run = run_bulk_with_depth(&bulk, 2);

	assert_eq!(run.code, 0);
	assert!(run.outputs.is_empty(), "{:?}", run.outputs);

	let report = fs::read_to_string(base.join("report.txt")).unwrap();

	assert!(report.contains("inside more than 2 archives"), "{report}");
	assert!(report.contains("bulk_scan_depth"), "{report}");

	// A higher depth opens it
	let run = run_bulk_with_depth(&options(base), 3);

	assert!(
		base.join("squashed_top/a1/a2/a3.zip").is_file(),
		"{:?}",
		run.outputs
	);
}

#[test]
fn the_most_archives_inside_each_other_is_bounded_whatever_the_depth_is() {
	assert_eq!(nesting_limit(4), 4);
	assert_eq!(nesting_limit(32), 32);
	assert_eq!(nesting_limit(256), MAX_NESTING);
}

#[test]
fn a_failed_pack_inside_an_archive_inside_an_archive_does_not_stop_the_others() {
	let root = TempDir::new().unwrap();
	let base = root.path();

	let bad = tar_files_bytes(&[
		(
			"pack.mcmeta",
			br#"{"pack":{"pack_format":34,"description":"bad"}}"#.to_vec()
		),
		("assets/minecraft/a.txt", b"hello".to_vec())
	]);

	write_tar_files(
		&base.join("big.tar"),
		&[("a_bad.tar", bad), ("b_good.tar", tar_bytes(&[""]))]
	);

	let run = run_bulk(&options(base));

	assert_eq!(run.code, 1);
	assert!(base.join("squashed_big/b_good.zip").is_file());
	assert!(!base.join("squashed_big/a_bad.zip").exists());
}
