use std::{cell::RefCell, fs, path::Path};

use tempfile::TempDir;

use super::*;

fn options(root: &Path) -> BulkOptions {
	BulkOptions {
		input_directory: root.to_path_buf(),
		output_prefix: "squashed_".to_string(),
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

struct Run {
	code: i32,
	/// The result files that the squash function was asked to write.
	outputs: Vec<PathBuf>
}

/// Runs the bulk mode with a squash function that writes a small result file. A pack with `bad` in the description
/// fails. The function checks that the pack exists when it is called, because the files of an archive are deleted after.
fn run_bulk(options: &BulkOptions) -> Run {
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

	let code = run(options, 4, ExtractLimits::default(), &table, &mut squash);

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
	let input = Path::new("in").join("dir").join("Pack.7z");

	assert_eq!(
		output_path(&bulk, &input, "Pack", None),
		Path::new("in/dir/squashed_Pack.zip")
	);
	assert_eq!(
		output_path(&bulk, &input, "Pack", Some("sub/inner")),
		Path::new("in/dir/squashed_Pack_sub_inner.zip")
	);

	bulk.output_prefix = String::new();
	bulk.output_suffix = "_small".to_string();

	assert_eq!(
		output_path(&bulk, &input, "Pack", None),
		Path::new("in/dir/Pack_small.zip")
	);
}

#[test]
fn earlier_results_are_recognized() {
	let mut bulk = options(Path::new("in"));

	assert!(looks_like_output(&bulk, "squashed_a"));
	assert!(!looks_like_output(&bulk, "a"));

	bulk.output_prefix = String::new();
	bulk.output_suffix = "_small".to_string();

	assert!(looks_like_output(&bulk, "a_small"));
	assert!(!looks_like_output(&bulk, "a"));

	bulk.output_suffix = String::new();

	assert!(
		!looks_like_output(&bulk, "anything"),
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

	let found = discover(&options(base), 4);
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

	assert!(discover(&bulk, 4).is_empty());

	bulk.scan_depth = 4;

	assert_eq!(discover(&bulk, 4).len(), 2);

	// Directly in the input directory, a pack or an archive is at level 1
	write_tar(&root.path().join("top.tar"), &["Inner"]);
	bulk.scan_depth = 1;

	assert_eq!(discover(&bulk, 4).len(), 1);
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
	assert!(base.join("squashed_two_a.zip").is_file());
	assert!(base.join("squashed_two_sub_b.zip").is_file());
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
	bulk.output_prefix = String::new();

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
	assert!(base.join("squashed_two_a.zip").is_file());
	assert!(base.join("squashed_two_sub_b.zip").is_file());
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
