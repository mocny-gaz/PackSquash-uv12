use std::fs;
use std::path::Path;

use flate2::Compression;
use serde_json::json;
use spng::{ContextFlags, DecodeFlags, Format};

use super::*;
use crate::mcchanges::ZipArchive;
use crate::mcchanges::tests::{png, temp_dir, write_file};
use crate::config::{GlobalOptions, SquashOptions, ZipSpecConformanceLevel};
use crate::vfs::os_fs::OsFilesystem;
use crate::{PackSquasher, PackSquasherError, PackSquasherStatus};

const UUID_A: &str = "5c3a7b52-8a8e-4a43-9b6b-1b7a2c1d0e11";
const UUID_B: &str = "0f1d5a2e-3c4b-4d5e-8f60-7a8b9c0d1e2f";

const MANIFEST: &str = r#"{
	// Comments are accepted by the game
	"format_version": 2,
	"header": {
		"name": "Test pack",
		"description": "A test pack",
		"uuid": "5c3a7b52-8a8e-4a43-9b6b-1b7a2c1d0e11",
		"version": [1, 0, 0],
		"min_engine_version": [1, 21, 0]
	},
	"modules": [
		{
			"type": "resources",
			"uuid": "0f1d5a2e-3c4b-4d5e-8f60-7a8b9c0d1e2f",
			"version": [1, 0, 0]
		}
	]
}"#;

fn valid_manifest() -> serde_json::Value {
	json!({
		"format_version": 2,
		"header": {
			"name": "Test pack",
			"uuid": UUID_A,
			"version": [1, 0, 0],
			"min_engine_version": [1, 21, 0]
		},
		"modules": [{"type": "resources", "uuid": UUID_B, "version": [1, 0, 0]}]
	})
}

fn rgba_pixels(png: &[u8]) -> Vec<u8> {
	let mut reader = spng::Decoder::new(png)
		.with_decode_flags(DecodeFlags::TRANSPARENCY)
		.with_context_flags(ContextFlags::IGNORE_ADLER32)
		.with_output_format(Format::Rgba8)
		.read_info()
		.unwrap();
	let mut pixels = vec![0; reader.output_buffer_size()];
	reader.next_frame(&mut pixels).unwrap();
	pixels
}

#[test]
fn valid_manifests_are_accepted() {
	assert_eq!(validate_manifest_value(&valid_manifest()).unwrap(), Vec::<String>::new());

	// Semantic version strings are allowed in manifests with format version 2
	let mut manifest = valid_manifest();
	manifest["header"]["version"] = json!("1.2.3-beta+build");
	manifest["modules"][0]["version"] = json!("2.0.0");
	assert!(validate_manifest_value(&manifest).unwrap().is_empty());

	// Dependencies and extra keys are not a concern
	manifest["dependencies"] = json!([{"uuid": UUID_B, "version": [1, 0, 0]}]);
	manifest["metadata"] = json!({"authors": ["Someone"]});
	assert!(validate_manifest_value(&manifest).unwrap().is_empty());
}

#[test]
fn suspicious_manifests_get_warnings() {
	let mut manifest = valid_manifest();
	manifest["header"].as_object_mut().unwrap().remove("min_engine_version");
	assert_eq!(validate_manifest_value(&manifest).unwrap().len(), 1);

	let mut manifest = valid_manifest();
	manifest["modules"][0]["type"] = json!("data");
	assert_eq!(validate_manifest_value(&manifest).unwrap().len(), 1);

	let mut manifest = valid_manifest();
	manifest["format_version"] = json!(3);
	assert_eq!(validate_manifest_value(&manifest).unwrap().len(), 1);
}

#[test]
fn invalid_manifests_are_rejected() {
	type Mutation = (&'static str, fn(&mut serde_json::Value));
	let mutations: [Mutation; 13] = [
		("not an object", |m| *m = json!([])),
		("missing format version", |m| {
			m.as_object_mut().unwrap().remove("format_version");
		}),
		("textual format version", |m| m["format_version"] = json!("2")),
		("missing header", |m| {
			m.as_object_mut().unwrap().remove("header");
		}),
		("bad header UUID", |m| m["header"]["uuid"] = json!("not-a-uuid")),
		("missing name", |m| {
			m["header"].as_object_mut().unwrap().remove("name");
		}),
		("empty name", |m| m["header"]["name"] = json!("")),
		("short version", |m| m["header"]["version"] = json!([1, 0])),
		("negative version", |m| m["header"]["version"] = json!([1, -1, 0])),
		("no modules", |m| m["modules"] = json!([])),
		("module UUID equals header UUID", |m| m["modules"][0]["uuid"] = json!(UUID_A)),
		("module without type", |m| {
			m["modules"][0].as_object_mut().unwrap().remove("type");
		}),
		("bad min engine version", |m| m["header"]["min_engine_version"] = json!("new"))
	];

	for (name, mutate) in mutations {
		let mut manifest = valid_manifest();
		mutate(&mut manifest);

		assert!(
			matches!(
				validate_manifest_value(&manifest),
				Err(BedrockManifestError::InvalidManifest(_))
			),
			"{name}"
		);
	}

	// Semantic version strings are only for format version 2
	let mut manifest = valid_manifest();
	manifest["format_version"] = json!(1);
	manifest["header"]["version"] = json!("1.0.0");
	assert!(validate_manifest_value(&manifest).is_err());

	// Repeated module UUIDs
	let mut manifest = valid_manifest();
	manifest["modules"] = json!([
		{"type": "resources", "uuid": UUID_B, "version": [1, 0, 0]},
		{"type": "data", "uuid": UUID_B.to_uppercase(), "version": [1, 0, 0]}
	]);
	assert!(validate_manifest_value(&manifest).is_err());
}

#[test]
fn files_are_classified_for_bedrock() {
	for (path, skip_pack_icon, expected) in [
		("manifest.json", false, BedrockFileKind::Passthrough),
		("contents.json", false, BedrockFileKind::Passthrough),
		("pack_icon.png", false, BedrockFileKind::Texture),
		("bug_pack_icon.png", false, BedrockFileKind::Texture),
		("pack_icon.png", true, BedrockFileKind::ExcludedPackIcon),
		("blocks.json", false, BedrockFileKind::Json),
		("sounds.json", false, BedrockFileKind::Json),
		("biomes_client.json", false, BedrockFileKind::Json),
		("README.txt", false, BedrockFileKind::Unknown),
		("packsquash.toml", false, BedrockFileKind::Unknown),
		("textures/blocks/stone.png", false, BedrockFileKind::Texture),
		("textures/blocks/STONE.PNG", false, BedrockFileKind::Texture),
		("textures/blocks/stone.tga", false, BedrockFileKind::Passthrough),
		("textures/terrain_texture.json", false, BedrockFileKind::Json),
		("models/entity/cow.geo.json", false, BedrockFileKind::Json),
		("sounds/mob/cow.ogg", false, BedrockFileKind::OggAudio),
		("sounds/mob/cow.wav", false, BedrockFileKind::Passthrough),
		("sounds/mob/cow.fsb", false, BedrockFileKind::Passthrough),
		("texts/en_US.lang", false, BedrockFileKind::Passthrough),
		("materials/entity.material", false, BedrockFileKind::Passthrough),
		("subpacks/low/textures/a.png", false, BedrockFileKind::Texture),
		("textures/.hidden", false, BedrockFileKind::Passthrough),
		// Icons are only special at the root
		("textures/pack_icon.png", true, BedrockFileKind::Texture)
	] {
		assert_eq!(BedrockFileKind::of(path, skip_pack_icon), expected, "{path}");
	}
}

#[test]
fn edition_is_detected_from_the_pack_files() {
	let pack_dir = temp_dir();
	let detect = |edition| is_bedrock_pack(&OsFilesystem, pack_dir.path(), edition);

	assert!(!detect(PackEdition::Auto), "empty directory");

	write_file(pack_dir.path(), "manifest.json", MANIFEST.as_bytes());
	assert!(detect(PackEdition::Auto), "manifest only");

	write_file(pack_dir.path(), "pack.mcmeta", b"{}");
	assert!(!detect(PackEdition::Auto), "manifest and pack.mcmeta");

	assert!(detect(PackEdition::Bedrock));
	assert!(!detect(PackEdition::Java));
}

fn bedrock_pack(pack_dir: &Path) {
	write_file(pack_dir, "manifest.json", MANIFEST.as_bytes());

	let pixels: [u8; 64] = std::array::from_fn(|i| match i % 4 {
		// A fully transparent pixel every other pixel, with color data that must survive
		3 if (i / 4) % 2 == 0 => 0,
		3 => 255,
		_ => (i * 7 % 251) as u8
	});
	let texture = png(4, 4, &pixels, Compression::none());
	write_file(pack_dir, "pack_icon.png", &texture);
	write_file(pack_dir, "textures/blocks/stone.png", &texture);

	write_file(
		pack_dir,
		"textures/terrain_texture.json",
		b"{\n\t// A comment\n\t\"z_key\": [1, 2.50, 3e2],\n\t\"a_key\": {\"nested\": true} /* inline */\n}\n"
	);
	write_file(pack_dir, "texts/en_US.lang", b"## Comment\n\nitem.a.name=A\t## inline\n");
	write_file(pack_dir, "materials/entity.material", b"\x00binary material\xFF");
	write_file(pack_dir, "textures/blocks/stone.tga", b"tga data");
	write_file(pack_dir, "README.txt", b"not part of the pack");
	write_file(pack_dir, "packsquash.toml", b"pack_directory = '.'");
}

struct SquashResult {
	output: Option<ZipArchive>,
	statuses: Vec<(String, bool, bool)>,
	result: Result<(), PackSquasherError>
}

fn squash(pack_dir: &Path, edit: impl FnOnce(&mut SquashOptions)) -> SquashResult {
	let output_dir = temp_dir();
	let output_path = output_dir.path().join("pack.mcpack");

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
	let result = PackSquasher::new().run(OsFilesystem, options, Some(sender));

	let mut statuses = Vec::new();
	while let Ok(status) = receiver.try_recv() {
		if let PackSquasherStatus::PackFileProcessed(status) = status {
			statuses.push((
				status.path().as_str().to_owned(),
				status.skipped(),
				status.intentionally_skipped()
			));
		}
	}

	SquashResult {
		output: result
			.is_ok()
			.then(|| ZipArchive::parse(fs::read(output_path).unwrap()).unwrap()),
		statuses,
		result
	}
}

/// Not async because `PackSquasher::run` starts its own runtime.
#[test]
fn bedrock_packs_are_squashed_with_bedrock_rules() {
	let pack_dir = temp_dir();
	bedrock_pack(pack_dir.path());

	let SquashResult {
		output,
		statuses,
		result
	} = squash(pack_dir.path(), |_| {});
	result.expect("Squashing a valid Bedrock pack should succeed");
	let output = output.unwrap();

	// Copied as-is, so comments, formatting and unusual bytes stay
	for file in [
		"manifest.json",
		"texts/en_US.lang",
		"materials/entity.material",
		"textures/blocks/stone.tga"
	] {
		assert_eq!(
			output.read(file).as_deref(),
			fs::read(pack_dir.path().join(file)).ok().as_deref(),
			"{file}"
		);
	}

	// JSON is minified, comments are removed, and key order and numbers are kept
	assert_eq!(
		String::from_utf8(output.read("textures/terrain_texture.json").unwrap()).unwrap(),
		r#"{"z_key":[1,2.5,300.0],"a_key":{"nested":true}}"#
	);

	// Textures are optimized, but the color of transparent pixels is not touched
	let original = fs::read(pack_dir.path().join("textures/blocks/stone.png")).unwrap();
	for file in ["pack_icon.png", "textures/blocks/stone.png"] {
		let optimized = output.read(file).unwrap();
		assert!(optimized.len() < original.len(), "{file} should be smaller");
		assert_eq!(rgba_pixels(&optimized), rgba_pixels(&original), "{file}");
	}

	// Files that are not part of Bedrock packs are skipped, and reported without being
	// marked as intentional
	for file in ["README.txt", "packsquash.toml"] {
		assert!(output.read(file).is_none(), "{file}");
		assert!(statuses.contains(&(file.to_owned(), true, false)), "{file}");
	}
}

#[test]
fn bedrock_json_numbers_keep_their_values() {
	// Documents what the JSON serializer does with number notations, to make sure no
	// value changes. 3e2 is written as 300.0, and 2.50 as 2.5
	let value: serde_json::Value = serde_json::from_str("[1, 2.50, 3e2, 1E-3, 100000000000]").unwrap();
	let reparsed: serde_json::Value =
		serde_json::from_str(&serde_json::to_string(&value).unwrap()).unwrap();

	assert_eq!(value, reparsed);
	assert_eq!(
		serde_json::to_string(&value).unwrap(),
		"[1,2.5,300.0,0.001,100000000000]"
	);
}

#[test]
fn the_pack_icon_can_be_left_out() {
	let pack_dir = temp_dir();
	bedrock_pack(pack_dir.path());

	let SquashResult {
		output, statuses, ..
	} = squash(pack_dir.path(), |options| {
		options.global_options.skip_pack_icon = true
	});

	assert!(output.unwrap().read("pack_icon.png").is_none());
	assert!(statuses.contains(&("pack_icon.png".to_owned(), true, true)));
}

#[test]
fn invalid_bedrock_options_and_manifests_are_rejected() {
	let pack_dir = temp_dir();
	bedrock_pack(pack_dir.path());

	let SquashResult { result, .. } = squash(pack_dir.path(), |options| {
		options.global_options.zip_spec_conformance_level = ZipSpecConformanceLevel::Balanced
	});
	assert!(matches!(result, Err(PackSquasherError::InvalidOptions(_))));

	write_file(pack_dir.path(), "manifest.json", br#"{"format_version": 2}"#);
	let SquashResult { result, .. } = squash(pack_dir.path(), |_| {});
	assert!(matches!(
		result,
		Err(PackSquasherError::BedrockManifestError(
			BedrockManifestError::InvalidManifest(_)
		))
	));

	// Validation can be turned off
	let SquashResult { result, .. } = squash(pack_dir.path(), |options| {
		options.global_options.validate_pack_metadata_file = false
	});
	result.expect("Skipping manifest validation should allow squashing");
}

#[test]
fn lenient_json_can_be_copied_as_is_on_request() {
	let pack_dir = temp_dir();
	bedrock_pack(pack_dir.path());
	// Valid for some tools, but not JSON: it has a trailing comma
	let lenient_json = b"{\"a\": [1, 2,],}";
	write_file(pack_dir.path(), "entity/lenient.json", lenient_json);

	let SquashResult { result, .. } = squash(pack_dir.path(), |_| {});
	assert!(result.is_err(), "invalid JSON should be reported");

	let SquashResult { output, result, .. } = squash(pack_dir.path(), |options| {
		options.file_options.insert(
			"entity/lenient.json".to_owned(),
			crate::config::FileOptions::CustomFileOptions(crate::config::CustomFileOptions {
				force_include: true,
				..Default::default()
			})
		);
	});
	result.expect("Forcing the file to be included should allow squashing");
	assert_eq!(
		output.unwrap().read("entity/lenient.json").as_deref(),
		Some(&lenient_json[..])
	);
}

#[test]
fn java_packs_are_not_affected_by_a_manifest_file() {
	let pack_dir = temp_dir();
	write_file(pack_dir.path(), "manifest.json", MANIFEST.as_bytes());
	write_file(
		pack_dir.path(),
		"pack.mcmeta",
		br#"{"pack": {"pack_format": 1, "description": "Java pack"}}"#
	);
	write_file(pack_dir.path(), "assets/minecraft/lang/en_US.lang", b"a=1\n");

	let SquashResult {
		output, statuses, ..
	} = squash(pack_dir.path(), |_| {});
	let output = output.unwrap();

	assert!(output.read("assets/minecraft/lang/en_US.lang").is_some());
	// Java Edition rules skip files they don't know, like the manifest
	assert!(output.read("manifest.json").is_none());
	assert!(statuses.contains(&("manifest.json".to_owned(), true, false)));
}
