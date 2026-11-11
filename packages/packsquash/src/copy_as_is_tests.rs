use std::fs;
use std::path::Path;

use flate2::Compression;

use crate::config::{CustomFileOptions, FileOptions, GlobalOptions, SquashOptions};
use crate::mcchanges::ZipArchive;
use crate::mcchanges::tests::{png, temp_dir, write_file};
use crate::vfs::os_fs::OsFilesystem;
use crate::{PackSquasher, PackSquasherError, PackSquasherStatus};

fn parse(options: &str) -> FileOptions {
	serde_json::from_str(options).expect("The file options should be valid")
}

#[test]
fn copy_as_is_is_parsed_without_confusing_other_options() {
	assert!(matches!(
		parse(r#"{"copy_as_is": true}"#),
		FileOptions::CustomFileOptions(CustomFileOptions {
			copy_as_is: true,
			force_include: false,
			..
		})
	));
	assert!(matches!(
		parse(r#"{"force_include": true}"#),
		FileOptions::CustomFileOptions(CustomFileOptions {
			copy_as_is: false,
			force_include: true,
			..
		})
	));
	assert!(matches!(
		parse(r#"{"copy_as_is": false, "force_include": true}"#),
		FileOptions::CustomFileOptions(CustomFileOptions {
			copy_as_is: false,
			force_include: true,
			..
		})
	));

	// Options of other file types are not taken for custom file options
	assert!(matches!(
		parse(r#"{"minify_json": false}"#),
		FileOptions::JsonFileOptions(_)
	));
	assert!(!matches!(parse("{}"), FileOptions::CustomFileOptions(_)));

	// Mixing options of different kinds is an error
	assert!(
		serde_json::from_str::<FileOptions>(r#"{"copy_as_is": true, "minify_json": false}"#).is_err()
	);
}

const MODEL: &[u8] = b"{\n\t\"parent\": \"block/cube_all\",\n\t\"textures\": {\n\t\t\"all\": \"blocks/stone\"\n\t}\n}\n";
const LANGUAGE: &[u8] = b"# A comment\n\nitem.a.name=A\n\nitem.b.name=B\n";
const README: &[u8] = b"not a Minecraft file";

fn pack(pack_dir: &Path) -> Vec<u8> {
	let texture = png(
		2,
		2,
		&[1, 2, 3, 255, 1, 2, 3, 255, 1, 2, 3, 255, 1, 2, 3, 255],
		Compression::none()
	);

	write_file(
		pack_dir,
		"pack.mcmeta",
		br#"{"pack": {"pack_format": 1, "description": "Test pack"}}"#
	);
	write_file(pack_dir, "assets/minecraft/models/block/a.json", MODEL);
	write_file(pack_dir, "assets/minecraft/models/block/b.json", MODEL);
	write_file(
		pack_dir,
		"assets/minecraft/textures/blocks/stone.png",
		&texture
	);
	write_file(
		pack_dir,
		"assets/minecraft/textures/blocks/dirt.png",
		&texture
	);
	write_file(pack_dir, "assets/minecraft/lang/en_US.lang", LANGUAGE);
	write_file(pack_dir, "assets/minecraft/custom/readme.txt", README);

	texture
}

struct Squashed {
	output: ZipArchive,
	statuses: Vec<(String, bool)>
}

/// Not async because `PackSquasher::run` starts its own runtime.
fn squash(pack_dir: &Path, edit: impl FnOnce(&mut SquashOptions)) -> Squashed {
	try_squash(pack_dir, edit).expect("Squashing a valid pack should succeed")
}

/// Like [`squash`], but returns the error if the operation fails.
fn try_squash(
	pack_dir: &Path,
	edit: impl FnOnce(&mut SquashOptions)
) -> Result<Squashed, PackSquasherError> {
	let output_dir = temp_dir();
	let output_path = output_dir.path().join("pack.zip");

	let mut options = SquashOptions {
		pack_directory: pack_dir.to_path_buf(),
		global_options: GlobalOptions {
			output_file_path: output_path.clone(),
			..GlobalOptions::default()
		},
		file_options: Default::default()
	};
	options.global_options.zip_compression_iterations = 1;
	edit(&mut options);

	let (sender, mut receiver) = tokio::sync::mpsc::channel(64);
	PackSquasher::new().run(OsFilesystem, options, Some(sender))?;

	let mut statuses = Vec::new();
	while let Ok(status) = receiver.try_recv() {
		if let PackSquasherStatus::PackFileProcessed(status) = status {
			statuses.push((status.path().as_str().to_owned(), status.skipped()));
		}
	}

	Ok(Squashed {
		output: ZipArchive::parse(fs::read(output_path).unwrap()).unwrap(),
		statuses
	})
}

fn copy_as_is() -> FileOptions {
	FileOptions::CustomFileOptions(CustomFileOptions {
		copy_as_is: true,
		..Default::default()
	})
}

#[test]
fn files_can_be_copied_as_is_even_if_packsquash_knows_them() {
	let pack_dir = temp_dir();
	let texture = pack(pack_dir.path());

	let Squashed { output, statuses } = squash(pack_dir.path(), |options| {
		for glob in [
			"assets/minecraft/models/block/a.json",
			"assets/minecraft/textures/blocks/stone.png",
			"assets/minecraft/lang/en_US.lang",
			"assets/minecraft/custom/**"
		] {
			options.file_options.insert(glob.to_owned(), copy_as_is());
		}
	});

	let read = |path: &str| output.read(&format!("assets/minecraft/{path}"));

	// The files that were asked to be copied have exactly the same bytes
	assert_eq!(read("models/block/a.json").as_deref(), Some(MODEL));
	assert_eq!(
		read("textures/blocks/stone.png").as_deref(),
		Some(&texture[..])
	);
	assert_eq!(read("lang/en_US.lang").as_deref(), Some(LANGUAGE));
	// This includes the files that PackSquash does not know, like with force_include
	assert_eq!(read("custom/readme.txt").as_deref(), Some(README));

	// The others are optimized as usual
	assert!(read("models/block/b.json").unwrap().len() < MODEL.len());
	assert!(read("textures/blocks/dirt.png").unwrap().len() < texture.len());

	assert!(statuses.iter().all(|(_, skipped)| !skipped));
}

#[test]
fn copied_files_keep_their_name_and_are_not_changed_by_other_options() {
	let pack_dir = temp_dir();
	pack(pack_dir.path());

	let mcchanges_dir = temp_dir();
	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/lang/en_US.lang",
		b"item.a.name=A\n"
	);
	write_file(
		mcchanges_dir.path(),
		"assets/minecraft/lang/fr_FR.lang",
		b"item.a.name=A\n"
	);
	write_file(
		pack_dir.path(),
		"assets/minecraft/lang/fr_FR.lang",
		b"item.a.name=A\nitem.b.name=B\n"
	);

	let Squashed { output, .. } = squash(pack_dir.path(), |options| {
		options.global_options.mcchanges_path = Some(mcchanges_dir.path().to_path_buf());
		options
			.file_options
			.insert("assets/minecraft/lang/en_US.lang".to_owned(), copy_as_is());
	});

	// Entries identical to the mcchanges pack are removed, except in the file that is copied as-is
	assert_eq!(
		output.read("assets/minecraft/lang/en_US.lang").as_deref(),
		Some(LANGUAGE)
	);
	let french = String::from_utf8(output.read("assets/minecraft/lang/fr_FR.lang").unwrap()).unwrap();
	assert!(
		french.contains("item.b.name=B") && !french.contains("item.a.name"),
		"{french}"
	);
}

#[test]
fn unrecognized_asset_files_are_kept_only_if_asked() {
	for keep in [false, true] {
		let pack_dir = temp_dir();
		pack(pack_dir.path());
		write_file(
			pack_dir.path(),
			"assets/minecraft/some_dir/data.dat",
			b"mod data"
		);
		write_file(
			pack_dir.path(),
			"assets/some_mod/anything/file.xyz",
			b"mod binary"
		);
		write_file(
			pack_dir.path(),
			"assets/some_mod/textures/block/x.dat",
			b"odd extension"
		);
		write_file(
			pack_dir.path(),
			"assets/loose.txt",
			b"not in a directory of assets"
		);
		write_file(pack_dir.path(), "outside/file.txt", b"outside");

		let Squashed { output, statuses } = squash(pack_dir.path(), |options| {
			options.global_options.include_unrecognized_asset_files = keep;
		});

		// Files in a directory of assets, in any namespace, are copied as they are
		for (path, content) in [
			("assets/minecraft/some_dir/data.dat", &b"mod data"[..]),
			("assets/minecraft/custom/readme.txt", README),
			("assets/some_mod/anything/file.xyz", b"mod binary"),
			("assets/some_mod/textures/block/x.dat", b"odd extension")
		] {
			assert_eq!(
				output.read(path).as_deref(),
				keep.then_some(content),
				"{path}, keep: {keep}"
			);
		}

		// The others are skipped as usual
		for path in ["assets/loose.txt", "outside/file.txt"] {
			assert!(output.read(path).is_none(), "{path}, keep: {keep}");
		}

		// The files that PackSquash knows are optimized as usual in both cases
		assert!(
			output
				.read("assets/minecraft/models/block/b.json")
				.unwrap()
				.len()
				< MODEL.len(),
			"keep: {keep}"
		);
		assert_eq!(
			statuses
				.iter()
				.filter(|(path, skipped)| *skipped && path.starts_with("assets/"))
				.count(),
			// Only the one that is not in a directory of assets, when the files are kept
			if keep { 1 } else { 5 },
			"keep: {keep}: {statuses:?}"
		);
	}
}

#[test]
fn files_that_fail_to_optimize_are_copied_as_they_are_unless_asked_not_to() {
	let broken_ogg = [b"OggS".as_slice(), &[0; 60]].concat();
	let broken_png = [b"\x89PNG\r\n\x1a\n".as_slice(), b"not really a PNG"].concat();
	let not_a_png = b"JFIF this is not a PNG at all".to_vec();
	let broken_json = b"{ this is not json".to_vec();

	for copy_failed_files in [true, false] {
		let pack_dir = temp_dir();
		pack(pack_dir.path());
		write_file(
			pack_dir.path(),
			"assets/minecraft/sounds/mob/bad.ogg",
			&broken_ogg
		);
		write_file(
			pack_dir.path(),
			"assets/minecraft/textures/blocks/bad.png",
			&broken_png
		);
		write_file(
			pack_dir.path(),
			"assets/minecraft/textures/blocks/jpeg.png",
			&not_a_png
		);
		write_file(
			pack_dir.path(),
			"assets/some_mod/models/bad.json",
			&broken_json
		);

		let result = try_squash(pack_dir.path(), |options| {
			options.global_options.copy_failed_files = copy_failed_files;
		});

		if !copy_failed_files {
			assert!(
				result.is_err(),
				"a file that can't be optimized fails the operation"
			);
			continue;
		}

		let Squashed { output, statuses } = result.expect("the files are copied as they are");

		for (path, content) in [
			("assets/minecraft/sounds/mob/bad.ogg", &broken_ogg),
			("assets/minecraft/textures/blocks/bad.png", &broken_png),
			("assets/minecraft/textures/blocks/jpeg.png", &not_a_png),
			("assets/some_mod/models/bad.json", &broken_json)
		] {
			assert_eq!(
				output.read(path).as_deref(),
				Some(content.as_slice()),
				"{path}"
			);
			assert!(
				statuses
					.iter()
					.any(|(status_path, skipped)| status_path == path && !skipped),
				"{path}: {statuses:?}"
			);
		}

		// The files that can be optimized still are
		assert!(
			output
				.read("assets/minecraft/models/block/b.json")
				.unwrap()
				.len()
				< MODEL.len()
		);
	}
}
