//! Support for Minecraft Bedrock Edition resource packs (`.mcpack` files, which are ZIP files).
//!
//! Bedrock packs have a different structure than Java Edition ones: they are identified by a
//! `manifest.json` file instead of `pack.mcmeta`, their files are not rooted under an `assets`
//! directory, and many of their formats differ subtly from the Java ones. For this reason, Bedrock
//! packs do not go through the Java asset type matching. Instead, files are classified by this
//! module, and then processed with the optimizers that are known to be safe for Bedrock.

use std::io;
use std::path::Path;

use json_comments::StripComments;
use serde_json::Value;
use thiserror::Error;

use crate::config::{CustomFileOptions, FileOptions, JsonFileOptions, PackEdition, PngFileOptions};
use crate::pack_file::asset_type::{PackFileAssetType, PackFileAssetTypeMatches};
use crate::vfs::{VirtualFileSystem, read_to_end};

/// The name of the file that identifies a Bedrock pack, located at its root.
pub(crate) const MANIFEST_FILE_NAME: &str = "manifest.json";

/// An error that may occur while reading and validating a Bedrock pack manifest.
#[derive(Error, Debug)]
#[non_exhaustive]
pub enum BedrockManifestError {
	/// Thrown when the manifest file can't be read.
	#[error("I/O error while reading the manifest.json file: {0}")]
	IoError(#[from] io::Error),
	/// Thrown when the manifest file is not valid JSON.
	#[error("The manifest.json file is not valid JSON: {0}")]
	JsonError(#[from] serde_json::Error),
	/// Thrown when the manifest file is valid JSON, but it does not follow the
	/// structure that Bedrock Edition expects.
	#[error("The manifest.json file is not valid: {0}")]
	InvalidManifest(String)
}

/// How a pack file of a Bedrock pack is going to be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BedrockFileKind {
	/// A PNG texture.
	Texture,
	/// A JSON file, which may contain comments.
	Json,
	/// An audio file in Ogg Vorbis format.
	OggAudio,
	/// A file that is copied to the ZIP file as-is.
	Passthrough,
	/// A file that is expected to be skipped, because it is not part of a Bedrock pack.
	/// It can still be included by explicitly asking for it in the file options.
	Unknown,
	/// The pack icon, which the user asked to leave out.
	ExcludedPackIcon
}

impl BedrockFileKind {
	/// Classifies a pack file according to its path relative to the root of the pack.
	pub(crate) fn of(relative_path: &str, skip_pack_icon: bool) -> Self {
		let Some((_, file_name)) = relative_path.rsplit_once('/') else {
			// A file at the root of the pack
			return match relative_path {
				"pack_icon.png" | "bug_pack_icon.png" if skip_pack_icon => Self::ExcludedPackIcon,
				// Keep the pack identity files readable and untouched
				MANIFEST_FILE_NAME | "contents.json" => Self::Passthrough,
				"pack_icon.png" | "bug_pack_icon.png" => Self::Texture,
				"blocks.json" | "sounds.json" | "biomes_client.json" => Self::Json,
				_ => Self::Unknown
			};
		};

		// The extension is whatever follows the last dot, but only if it is not a hidden
		// file name with no extension
		match file_name
			.rsplit_once('.')
			.filter(|(name, _)| !name.is_empty())
			.map(|(_, extension)| extension.to_ascii_lowercase())
			.as_deref()
		{
			Some("png") => Self::Texture,
			Some("json") => Self::Json,
			Some("ogg") => Self::OggAudio,
			// Language files are copied as-is on purpose. Their comment syntax has subtle
			// rules, and the Java Edition language file optimizer would reject valid
			// Bedrock files. Other formats (TGA, JPEG, WAV, FSB, materials...) have no
			// safe optimization, but they are still needed by the game
			_ => Self::Passthrough
		}
	}

	/// Returns the asset type matches to use for a pack file of this kind. A custom asset type
	/// match is always included so that users can ask for any file to be copied as-is.
	pub(crate) fn asset_type_matches(self) -> PackFileAssetTypeMatches {
		match self {
			Self::Texture => PackFileAssetTypeMatches::of_asset_types(&[
				PackFileAssetType::GenericTexture,
				PackFileAssetType::Custom
			]),
			// The comment-friendly asset type always accepts comments, and the file keeps
			// its `.json` extension
			Self::Json => PackFileAssetTypeMatches::of_asset_types(&[
				PackFileAssetType::GenericJsonWithComments,
				PackFileAssetType::Custom
			]),
			Self::OggAudio => PackFileAssetTypeMatches::of_asset_types(&[
				PackFileAssetType::GenericOggVorbisAudio,
				PackFileAssetType::Custom
			]),
			Self::Passthrough | Self::Unknown | Self::ExcludedPackIcon => {
				PackFileAssetTypeMatches::of_custom_asset_type()
			}
		}
	}

	/// Returns the file options to use for a pack file of this kind when the user did not
	/// provide any that apply to it.
	pub(crate) fn default_file_options(self) -> Option<FileOptions> {
		match self {
			Self::Texture => Some(FileOptions::PngFileOptions(PngFileOptions {
				// Bedrock uses the alpha channel for more than transparency in some textures,
				// such as the subsurface scattering of ray tracing textures and the height
				// of some normal maps. Changing the color of seemingly transparent pixels
				// would change those data
				skip_alpha_optimizations: true,
				..PngFileOptions::default()
			})),
			Self::Json => Some(FileOptions::JsonFileOptions(JsonFileOptions {
				// Keep the original order of the keys, which a tool or the game may rely on
				sort_object_keys: false,
				..JsonFileOptions::default()
			})),
			Self::OggAudio => Some(FileOptions::AudioFileOptions(Default::default())),
			Self::Passthrough => Some(FileOptions::CustomFileOptions(CustomFileOptions {
				force_include: true,
				..CustomFileOptions::default()
			})),
			Self::Unknown | Self::ExcludedPackIcon => None
		}
	}
}

/// Decides whether a pack is a Bedrock Edition pack, according to the requested edition. If
/// the edition is detected automatically, only packs that have a manifest, and no Java Edition
/// pack metadata file, are considered Bedrock Edition packs.
pub(crate) fn is_bedrock_pack<V: VirtualFileSystem>(
	vfs: &V,
	pack_directory: &Path,
	pack_edition: PackEdition
) -> bool {
	match pack_edition {
		PackEdition::Java => false,
		PackEdition::Bedrock => true,
		PackEdition::Auto => {
			let exists = |file_name: &str| {
				vfs.file_type(pack_directory.join(file_name))
					.is_ok_and(|file_type| !file_type.is_dir())
			};

			exists(MANIFEST_FILE_NAME) && !exists("pack.mcmeta") && !exists("pack.mcmetac")
		}
	}
}

/// Reads and validates the manifest of the Bedrock pack in the given directory. If it is
/// valid, a list of warnings about things that are probably mistakes is returned.
pub(crate) async fn validate_manifest<V: VirtualFileSystem>(
	vfs: &V,
	pack_directory: &Path
) -> Result<Vec<String>, BedrockManifestError> {
	let file = vfs.open(pack_directory.join(MANIFEST_FILE_NAME))?;
	let data = read_to_end(file.file_read, file.file_size_hint).await?;

	// The game accepts comments in JSON files, so be as lenient as it is
	let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&data);
	let manifest: Value = serde_json::from_reader(StripComments::new(data))?;

	validate_manifest_value(&manifest)
}

fn invalid(message: impl Into<String>) -> BedrockManifestError {
	BedrockManifestError::InvalidManifest(message.into())
}

/// Checks that a string is a UUID in its canonical hexadecimal form.
fn is_uuid(value: &str) -> bool {
	let groups = value.split('-').collect::<Vec<_>>();

	groups.len() == 5
		&& groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, length)| {
			group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
		})
}

/// Checks that a value is a valid version: an array of three non-negative integers or,
/// in manifests with format version 2, a semantic version string.
fn is_version(value: &Value, format_version: u64) -> bool {
	match value {
		Value::Array(parts) => parts.len() == 3 && parts.iter().all(Value::is_u64),
		Value::String(version) if format_version >= 2 => {
			let core = version.split(['-', '+']).next().unwrap_or_default();
			let parts = core.split('.').collect::<Vec<_>>();

			parts.len() == 3
				&& parts
					.iter()
					.all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
		}
		_ => false
	}
}

fn required_uuid<'a>(
	object: &'a serde_json::Map<String, Value>,
	location: &str
) -> Result<&'a str, BedrockManifestError> {
	object
		.get("uuid")
		.and_then(Value::as_str)
		.filter(|uuid| is_uuid(uuid))
		.ok_or_else(|| {
			invalid(format!(
				"{location} needs a \"uuid\" with the format 00000000-0000-0000-0000-000000000000"
			))
		})
}

fn validate_manifest_value(manifest: &Value) -> Result<Vec<String>, BedrockManifestError> {
	let mut warnings = Vec::new();

	let manifest = manifest
		.as_object()
		.ok_or_else(|| invalid("the root element must be an object"))?;

	let format_version = manifest
		.get("format_version")
		.and_then(Value::as_u64)
		.ok_or_else(|| invalid("\"format_version\" must be an integer, like 2"))?;
	if !(1..=2).contains(&format_version) {
		warnings.push(format!(
			"The manifest has format version {format_version}, which PackSquash does not know. The game may reject it"
		));
	}

	let header = manifest
		.get("header")
		.and_then(Value::as_object)
		.ok_or_else(|| invalid("\"header\" must be an object"))?;

	let header_uuid = required_uuid(header, "\"header\"")?;

	if !header
		.get("name")
		.and_then(Value::as_str)
		.is_some_and(|name| !name.is_empty())
	{
		return Err(invalid("\"header\" needs a non-empty \"name\" string"));
	}

	if !header
		.get("version")
		.is_some_and(|version| is_version(version, format_version))
	{
		return Err(invalid(
			"\"header\" needs a \"version\" with three numbers, like [1, 0, 0]"
		));
	}

	if !header.contains_key("min_engine_version") {
		warnings.push(
			"The manifest header has no \"min_engine_version\", which the game expects".to_owned()
		);
	} else if !header
		.get("min_engine_version")
		.is_some_and(|version| is_version(version, format_version))
	{
		return Err(invalid(
			"\"min_engine_version\" must have three numbers, like [1, 21, 0]"
		));
	}

	let modules = manifest
		.get("modules")
		.and_then(Value::as_array)
		.filter(|modules| !modules.is_empty())
		.ok_or_else(|| invalid("\"modules\" must be a list with at least one module"))?;

	let mut seen_uuids = vec![header_uuid.to_ascii_lowercase()];
	let mut has_resources_module = false;

	for (index, module) in modules.iter().enumerate() {
		let location = format!("module {}", index + 1);
		let module = module
			.as_object()
			.ok_or_else(|| invalid(format!("{location} must be an object")))?;

		let module_type = module
			.get("type")
			.and_then(Value::as_str)
			.ok_or_else(|| invalid(format!("{location} needs a \"type\" string")))?;
		has_resources_module |= matches!(module_type, "resources" | "client_data");

		let module_uuid = required_uuid(module, &location)?.to_ascii_lowercase();
		if seen_uuids.contains(&module_uuid) {
			// The game refuses to load packs that repeat UUIDs, including the one of the header
			return Err(invalid(format!(
				"the UUID of {location} is already used by the header or another module. Every UUID must be unique"
			)));
		}
		seen_uuids.push(module_uuid);

		if !module
			.get("version")
			.is_some_and(|version| is_version(version, format_version))
		{
			return Err(invalid(format!(
				"{location} needs a \"version\" with three numbers, like [1, 0, 0]"
			)));
		}
	}

	if !has_resources_module {
		warnings.push(
			"The manifest has no \"resources\" module. PackSquash is designed for resource packs, and processes other types of packs with the same rules without checking them".to_owned()
		);
	}

	Ok(warnings)
}

#[cfg(test)]
mod tests;
