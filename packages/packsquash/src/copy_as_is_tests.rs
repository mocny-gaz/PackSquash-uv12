use std::fs;
use std::path::Path;

use flate2::Compression;

use crate::config::{CustomFileOptions, FileOptions, GlobalOptions, SquashOptions};
use crate::mcchanges::ZipArchive;
use crate::mcchanges::tests::{png, temp_dir, write_file};
use crate::vfs::os_fs::OsFilesystem;
use crate::{PackSquasher, PackSquasherStatus};

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
	assert!(!matches!(
		parse("{}"),
		FileOptions::CustomFileOptions(_)
	));

	// Mixing options of different kinds is an error
	assert!(serde_json::from_str::<FileOptions>(r#"{"copy_as_is": true, "minify_json": false}"#).is_err());
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
	write_file(pack_dir, "assets/minecraft/textures/blocks/stone.png", &texture);
	write_file(pack_dir, "assets/minecraft/textures/blocks/dirt.png", &texture);
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
	PackSquasher::new()
		.run(OsFilesystem, options, Some(sender))
		.expect("Squashing a valid pack should succeed");

	let mut statuses = Vec::new();
	while let Ok(status) = receiver.try_recv() {
		if let PackSquasherStatus::PackFileProcessed(status) = status {
			statuses.push((status.path().as_str().to_owned(), status.skipped()));
		}
	}

	Squashed {
		output: ZipArchive::parse(fs::read(output_path).unwrap()).unwrap(),
		statuses
	}
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
	assert_eq!(read("textures/blocks/stone.png").as_deref(), Some(&texture[..]));
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
	write_file(mcchanges_dir.path(), "assets/minecraft/lang/en_US.lang", b"item.a.name=A\n");
	write_file(mcchanges_dir.path(), "assets/minecraft/lang/fr_FR.lang", b"item.a.name=A\n");
	write_file(pack_dir.path(), "assets/minecraft/lang/fr_FR.lang", b"item.a.name=A\nitem.b.name=B\n");

	let Squashed { output, .. } = squash(pack_dir.path(), |options| {
		options.global_options.mcchanges_path = Some(mcchanges_dir.path().to_path_buf());
		options.file_options.insert(
			"assets/minecraft/lang/en_US.lang".to_owned(),
			copy_as_is()
		);
	});

	// Entries identical to the mcchanges pack are removed, except in the file that is copied as-is
	assert_eq!(
		output.read("assets/minecraft/lang/en_US.lang").as_deref(),
		Some(LANGUAGE)
	);
	let french = String::from_utf8(output.read("assets/minecraft/lang/fr_FR.lang").unwrap()).unwrap();
	assert!(french.contains("item.b.name=B") && !french.contains("item.a.name"), "{french}");
}
