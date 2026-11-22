// Generated from the option list of the first window. Edit by hand from now on.

use super::model::{Kind, Opt};

pub const TABS: [&str; 6] = [
	"Pack",
	"Fork features",
	"ZIP file",
	"Images",
	"Audio",
	"Text and data"
];

pub const QUIRKS: [&str; 6] = [
	"grayscale_images_gamma_miscorrection",
	"restrictive_banner_layer_texture_format_check",
	"bad_entity_eye_layer_texture_transparency_blending",
	"java8_zip_parsing",
	"ogg_obfuscation_incompatibility",
	"png_obfuscation_incompatibility"
];

pub const TABLES: [(&str, &str, &str); 8] = [
	("png", "**/*.png", "PNG files"),
	("audio", "**/*.{ogg,oga,mp3,wav,flac}", "Audio files"),
	("json", "**/*.{json,mcmeta}", "JSON files"),
	("lang", "**/*.lang", "Legacy language files"),
	("fn", "**/*.mcfunction", "Function files"),
	("prop", "**/*.properties", "Properties files"),
	("nbt", "**/*.{nbt,dat}", "NBT files"),
	("shader", "**/*.{vsh,fsh,glsl}", "Shaders")
];

pub const OPTIONS: [Opt; 70] = [
	Opt {
		key: "pack_directory",
		tab: 0,
		kind: Kind::Text,
		default: "",
		hint: "Folder (or archive) with the pack. Required, unless you use bulk mode",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "output_file_path",
		tab: 0,
		kind: Kind::Text,
		default: "pack.zip",
		hint: "Where the squashed ZIP is written",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "pack_edition",
		tab: 0,
		kind: Kind::Choice,
		default: "auto",
		hint: "Java or Bedrock pack",
		choices: &["auto", "java", "bedrock"],
		range: (None, None),
		table: None
	},
	Opt {
		key: "zip_comment",
		tab: 0,
		kind: Kind::Text,
		default: "",
		hint: "Comment stored in the ZIP",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "log",
		tab: 0,
		kind: Kind::Bool,
		default: "false",
		hint: "Write a very detailed debug log file to send when something goes wrong",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "log_file",
		tab: 0,
		kind: Kind::Text,
		default: "",
		hint: "Where the debug log goes. Blank: packsquash-debug.log next to the options file",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "allow_mods",
		tab: 0,
		kind: Kind::Multi,
		default: "",
		hint: "Keep files that only these mods use",
		choices: &["OptiFine", "Minecraft Transit Railway 3"],
		range: (None, None),
		table: None
	},
	Opt {
		key: "skip_pack_icon",
		tab: 0,
		kind: Kind::Bool,
		default: "false",
		hint: "Leave pack.png out",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "validate_pack_metadata_file",
		tab: 0,
		kind: Kind::Bool,
		default: "true",
		hint: "Check pack.mcmeta",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "fix_pack_metadata",
		tab: 0,
		kind: Kind::Bool,
		default: "true",
		hint: "Repair pack.mcmeta when it fails the checks",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "ignore_system_and_hidden_files",
		tab: 0,
		kind: Kind::Bool,
		default: "true",
		hint: "Skip files like Thumbs.db and .DS_Store",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "automatic_minecraft_quirks_detection",
		tab: 0,
		kind: Kind::Bool,
		default: "true",
		hint: "Detect game version quirks from pack.mcmeta",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "work_around_minecraft_quirks",
		tab: 0,
		kind: Kind::Multi,
		default: "",
		hint: "Quirks to work around by hand (ignored while detection is on)",
		choices: &QUIRKS,
		range: (None, None),
		table: None
	},
	Opt {
		key: "automatic_asset_types_mask_detection",
		tab: 0,
		kind: Kind::Bool,
		default: "true",
		hint: "Skip file types the game version does not use",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "mcchanges_path",
		tab: 1,
		kind: Kind::Text,
		default: "",
		hint: "Jar with the game files’ hashes (mcchanges), or a folder that has mcchanges-java.jar with exactly that name. Files equal to the game’s are left out",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "mcchanges_version",
		tab: 1,
		kind: Kind::Text,
		default: "",
		hint: "Game version to compare with. Blank: try every version and use the best match",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "mcchanges_skip_lang",
		tab: 1,
		kind: Kind::Bool,
		default: "true",
		hint: "Drop language lines equal to the game’s",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "optifine_sky_unused",
		tab: 1,
		kind: Kind::Bool,
		default: "false",
		hint: "Drop OptiFine sky images that nothing uses",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "optifine_sky_merge",
		tab: 1,
		kind: Kind::Bool,
		default: "false",
		hint: "Store identical OptiFine sky images once",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "optifine_sky_fix",
		tab: 1,
		kind: Kind::Bool,
		default: "false",
		hint: "Drop OptiFine sky entries whose image is missing. Not for packs still being made",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "optifine_unused",
		tab: 1,
		kind: Kind::Bool,
		default: "false",
		hint: "Leave out OptiFine entries that are not connected to anything. Not for packs that are still being made",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "mcchanges_download",
		tab: 1,
		kind: Kind::Bool,
		default: "true",
		hint: "Find or download the mcchanges jar when mcchanges_path is not set. False turns it off",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "mcchanges_nowifi",
		tab: 1,
		kind: Kind::Bool,
		default: "false",
		hint: "Only search for the mcchanges jar, without the network",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "keep_unknown_files",
		tab: 1,
		kind: Kind::Bool,
		default: "false",
		hint: "Keep every file in a folder of assets, even if PackSquash does not know it (for mods)",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "archive_depth",
		tab: 1,
		kind: Kind::Int,
		default: "4",
		hint: "Archive input: folder levels searched for the pack inside an archive",
		choices: &[],
		range: (Some(1.0), Some(16.0)),
		table: None
	},
	Opt {
		key: "archive_max_mib",
		tab: 1,
		kind: Kind::Int,
		default: "16384",
		hint: "Archive input: stop if an archive holds more than this (MiB)",
		choices: &[],
		range: (Some(1.0), Some(1048576.0)),
		table: None
	},
	Opt {
		key: "bulk_input_directory",
		tab: 1,
		kind: Kind::Text,
		default: "",
		hint: "Bulk mode: squash every pack (folder or archive) found under this folder, or in this one archive file",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "bulk_output_directory",
		tab: 1,
		kind: Kind::Text,
		default: "",
		hint: "Bulk mode: put the results in this folder, with the same folders as the input and no prefix. Blank: next to each input",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "bulk_output_prefix",
		tab: 1,
		kind: Kind::Text,
		default: "squashed_",
		hint: "Bulk mode: start of each output name. With bulk_output_directory the results have no prefix unless you change this",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "bulk_output_suffix",
		tab: 1,
		kind: Kind::Text,
		default: "",
		hint: "Bulk mode: end of each output name, before .zip",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "bulk_overwrite",
		tab: 1,
		kind: Kind::Bool,
		default: "false",
		hint: "Bulk mode: replace the original pack with the squashed one",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "bulk_backup",
		tab: 1,
		kind: Kind::Bool,
		default: "true",
		hint: "Bulk mode: with overwrite, keep the original as NAME.original",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "bulk_dry_run",
		tab: 1,
		kind: Kind::Bool,
		default: "false",
		hint: "Bulk mode: only list what would happen",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "bulk_report_file",
		tab: 1,
		kind: Kind::Text,
		default: "",
		hint: "Bulk mode: also write the report to this file",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "bulk_archive_extensions",
		tab: 1,
		kind: Kind::Text,
		default: "zip,7z,rar,tar,gz,tgz,xz,txz,zst,tzst,bz2,tbz2",
		hint: "Bulk mode: file extensions that are opened as archives",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "bulk_scan_depth",
		tab: 1,
		kind: Kind::Int,
		default: "32",
		hint: "Bulk mode: folder levels searched, also inside archives",
		choices: &[],
		range: (Some(1.0), Some(256.0)),
		table: None
	},
	Opt {
		key: "recompress_compressed_files",
		tab: 2,
		kind: Kind::Bool,
		default: "false",
		hint: "Recompress files that are already compressed",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "zip_compression_iterations",
		tab: 2,
		kind: Kind::Int,
		default: "20",
		hint: "Zopfli passes, 0 to 255. More is smaller and slower",
		choices: &[],
		range: (Some(0.0), Some(255.0)),
		table: None
	},
	Opt {
		key: "zip_spec_conformance_level",
		tab: 2,
		kind: Kind::Choice,
		default: "pedantic",
		hint: "How strictly the ZIP follows the specification",
		choices: &["pedantic", "high", "balanced", "disregard"],
		range: (None, None),
		table: None
	},
	Opt {
		key: "size_increasing_zip_obfuscation",
		tab: 2,
		kind: Kind::Bool,
		default: "false",
		hint: "Make the ZIP harder to open, at a size cost",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "percentage_of_zip_structures_tuned_for_obfuscation_discretion",
		tab: 2,
		kind: Kind::Int,
		default: "0",
		hint: "Percent of ZIP structures tuned to hide the obfuscation",
		choices: &[],
		range: (Some(0.0), Some(100.0)),
		table: None
	},
	Opt {
		key: "never_store_squash_times",
		tab: 2,
		kind: Kind::Bool,
		default: "false",
		hint: "Do not store the times used to speed up later runs",
		choices: &[],
		range: (None, None),
		table: None
	},
	Opt {
		key: "threads",
		tab: 2,
		kind: Kind::Int,
		default: "",
		hint: "Worker threads. Blank: all physical threads",
		choices: &[],
		range: (Some(1.0), Some(1024.0)),
		table: None
	},
	Opt {
		key: "spooling_buffers_size",
		tab: 2,
		kind: Kind::Int,
		default: "",
		hint: "Spooling buffers size in bytes. Blank: half of the free memory",
		choices: &[],
		range: (Some(0.0), None),
		table: None
	},
	Opt {
		key: "image_data_compression_iterations",
		tab: 3,
		kind: Kind::Int,
		default: "5",
		hint: "PNG compression passes, 0 to 255",
		choices: &[],
		range: (Some(0.0), Some(255.0)),
		table: Some("png")
	},
	Opt {
		key: "color_quantization_target",
		tab: 3,
		kind: Kind::Choice,
		default: "auto",
		hint: "Reduce colors to a palette",
		choices: &[
			"none",
			"auto",
			"eight_bit_depth",
			"four_bit_depth",
			"two_bit_depth",
			"one_bit_depth"
		],
		range: (None, None),
		table: Some("png")
	},
	Opt {
		key: "color_quantization_dithering_level",
		tab: 3,
		kind: Kind::Float,
		default: "0.85",
		hint: "Dithering from 0 to 1",
		choices: &[],
		range: (Some(0.0), Some(1.0)),
		table: Some("png")
	},
	Opt {
		key: "maximum_width_and_height",
		tab: 3,
		kind: Kind::Int,
		default: "8192",
		hint: "Largest image side accepted",
		choices: &[],
		range: (Some(1.0), None),
		table: Some("png")
	},
	Opt {
		key: "skip_alpha_optimizations",
		tab: 3,
		kind: Kind::Bool,
		default: "false",
		hint: "Do not change invisible pixels",
		choices: &[],
		range: (None, None),
		table: Some("png")
	},
	Opt {
		key: "downsize_if_single_color",
		tab: 3,
		kind: Kind::Bool,
		default: "false",
		hint: "Shrink images that have one color",
		choices: &[],
		range: (None, None),
		table: Some("png")
	},
	Opt {
		key: "png_obfuscation",
		tab: 3,
		kind: Kind::Bool,
		default: "false",
		hint: "Obfuscate PNG files",
		choices: &[],
		range: (None, None),
		table: Some("png")
	},
	Opt {
		key: "transcode_ogg",
		tab: 4,
		kind: Kind::Bool,
		default: "true",
		hint: "Re-encode OGG files",
		choices: &[],
		range: (None, None),
		table: Some("audio")
	},
	Opt {
		key: "two_pass_vorbis_optimization_and_validation",
		tab: 4,
		kind: Kind::Bool,
		default: "true",
		hint: "Two encoding passes, and validation",
		choices: &[],
		range: (None, None),
		table: Some("audio")
	},
	Opt {
		key: "empty_audio_optimization",
		tab: 4,
		kind: Kind::Bool,
		default: "true",
		hint: "Replace silent audio with a tiny file",
		choices: &[],
		range: (None, None),
		table: Some("audio")
	},
	Opt {
		key: "channels",
		tab: 4,
		kind: Kind::Int,
		default: "",
		hint: "Channels. Blank: as the input",
		choices: &[],
		range: (Some(1.0), Some(2.0)),
		table: Some("audio")
	},
	Opt {
		key: "sampling_frequency",
		tab: 4,
		kind: Kind::Int,
		default: "",
		hint: "Sampling frequency in Hz. Blank: 40050 stereo, 32000 mono",
		choices: &[],
		range: (Some(1.0), None),
		table: Some("audio")
	},
	Opt {
		key: "bitrate_control_mode",
		tab: 4,
		kind: Kind::Choice,
		default: "CQF",
		hint: "Bitrate control mode",
		choices: &["CQF", "VBR", "ABR", "CABR"],
		range: (None, None),
		table: Some("audio")
	},
	Opt {
		key: "target_bitrate_control_metric",
		tab: 4,
		kind: Kind::Float,
		default: "",
		hint: "Target for the mode. Blank: 0.25 stereo, 0.0 mono",
		choices: &[],
		range: (Some(-2.0), None),
		table: Some("audio")
	},
	Opt {
		key: "target_pitch",
		tab: 4,
		kind: Kind::Float,
		default: "1",
		hint: "Pitch multiplier",
		choices: &[],
		range: (Some(0.0), None),
		table: Some("audio")
	},
	Opt {
		key: "ogg_obfuscation",
		tab: 4,
		kind: Kind::Bool,
		default: "false",
		hint: "Obfuscate OGG files",
		choices: &[],
		range: (None, None),
		table: Some("audio")
	},
	Opt {
		key: "minify_json",
		tab: 5,
		kind: Kind::Bool,
		default: "true",
		hint: "Remove whitespace from JSON",
		choices: &[],
		range: (None, None),
		table: Some("json")
	},
	Opt {
		key: "delete_bloat_keys",
		tab: 5,
		kind: Kind::Bool,
		default: "true",
		hint: "Remove keys the game ignores",
		choices: &[],
		range: (None, None),
		table: Some("json")
	},
	Opt {
		key: "always_allow_json_comments",
		tab: 5,
		kind: Kind::Bool,
		default: "true",
		hint: "Accept comments in JSON",
		choices: &[],
		range: (None, None),
		table: Some("json")
	},
	Opt {
		key: "sort_json_object_keys",
		tab: 5,
		kind: Kind::Bool,
		default: "true",
		hint: "Sort JSON keys (smaller ZIP)",
		choices: &[],
		range: (None, None),
		table: Some("json")
	},
	Opt {
		key: "minify_legacy_language",
		tab: 5,
		kind: Kind::Bool,
		default: "true",
		hint: "Minify .lang files",
		choices: &[],
		range: (None, None),
		table: Some("lang")
	},
	Opt {
		key: "strip_legacy_language_bom",
		tab: 5,
		kind: Kind::Bool,
		default: "true",
		hint: "Remove the BOM of .lang files",
		choices: &[],
		range: (None, None),
		table: Some("lang")
	},
	Opt {
		key: "minify_command_function",
		tab: 5,
		kind: Kind::Bool,
		default: "true",
		hint: "Minify .mcfunction files",
		choices: &[],
		range: (None, None),
		table: Some("fn")
	},
	Opt {
		key: "minify_properties",
		tab: 5,
		kind: Kind::Bool,
		default: "true",
		hint: "Minify .properties files",
		choices: &[],
		range: (None, None),
		table: Some("prop")
	},
	Opt {
		key: "nbt_compression_iterations",
		tab: 5,
		kind: Kind::Int,
		default: "15",
		hint: "NBT compression passes, 0 to 255",
		choices: &[],
		range: (Some(0.0), Some(255.0)),
		table: Some("nbt")
	},
	Opt {
		key: "shader_source_transformation_strategy",
		tab: 5,
		kind: Kind::Choice,
		default: "minify",
		hint: "What to do with shader source",
		choices: &["minify", "prettify", "keep_as_is"],
		range: (None, None),
		table: Some("shader")
	}
];
