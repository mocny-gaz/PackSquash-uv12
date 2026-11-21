//! Repairs the small mistakes that make a `pack.mcmeta` file fail the checks, so the pack can be
//! squashed and its game versions can be detected.

use std::{io::Read as _, path::Path};

use json_comments::StripComments;
use serde_json::Value;

use super::{PackMetadata, PackMetadataError, PackType};
use crate::{
	pack_file::{escape_control_characters_in_strings, strip_utf8_bom},
	vfs::VirtualFileSystem
};

mod syntax;

#[cfg(test)]
mod tests;

const FORMAT_KEYS: [&str; 7] = [
	"pack_format",
	"min_format",
	"max_format",
	"supported_formats",
	"formats",
	"min_inclusive",
	"max_inclusive"
];

/// A change made to the pack metadata file.
#[derive(Debug, PartialEq, Eq)]
pub struct MetadataFix {
	/// What was changed, in one short sentence.
	pub summary: String,
	/// The same, with the old and the new values.
	pub detail: String
}

/// What was fixed in a pack metadata file.
#[derive(Debug, Default)]
pub struct MetadataFixes {
	/// The new contents of the pack metadata file, if they have to change in the result.
	pub fixed_data: Option<Vec<u8>>,
	/// The size in bytes of the pack metadata file before the fixes.
	pub original_len: usize,
	pub fixes: Vec<MetadataFix>
}

impl PackMetadata {
	/// Reads the metadata for a pack at `root_path` on the given `vfs`, and repairs it if it
	/// does not pass the checks and the repair is known.
	pub async fn read_and_fix(
		vfs: &impl VirtualFileSystem,
		root_path: impl AsRef<Path>
	) -> Result<(Self, MetadataFixes), PackMetadataError> {
		let (pack_type, manifest) = Self::read_manifest(vfs, root_path).await?;

		match Self::parse(pack_type, &manifest) {
			Ok(pack_meta) => Ok((
				pack_meta,
				MetadataFixes {
					fixed_data: None,
					original_len: manifest.len(),
					fixes: raw_control_characters(&manifest).into_iter().collect()
				}
			)),
			Err(error @ PackMetadataError::JsonSerde(_)) => fix(pack_type, &manifest).ok_or(error),
			Err(error) => Err(error)
		}
	}
}

fn without_comments(manifest: &[u8]) -> Option<Vec<u8>> {
	let mut stripped = Vec::with_capacity(manifest.len());

	StripComments::new(strip_utf8_bom(manifest))
		.read_to_end(&mut stripped)
		.ok()?;

	Some(stripped)
}

/// The data is read as the game reads it, so a line break inside a string is fine, and the
/// minified result has it as an escape. This only tells about it.
fn raw_control_characters(manifest: &[u8]) -> Option<MetadataFix> {
	let stripped = without_comments(manifest)?;

	(escape_control_characters_in_strings(&stripped) != stripped).then(|| MetadataFix {
		summary: "a line break or a tab inside a string is written as an escape".into(),
		detail: "pack.mcmeta had a raw line break or tab inside a string, which the game accepts. The result has it as the escapes \\n and \\t".into()
	})
}

fn fix(pack_type: PackType, manifest: &[u8]) -> Option<(PackMetadata, MetadataFixes)> {
	let mut fixes = Vec::new();
	let stripped = without_comments(manifest)?;

	let mut value =
		match serde_json::from_slice::<Value>(&escape_control_characters_in_strings(&stripped)) {
			Ok(value) => value,
			Err(_) => {
				let (repaired, applied) = syntax::repair_json(manifest)?;

				fixes.extend(applied.into_iter().map(|what| MetadataFix {
					summary: what.into(),
					detail: format!(
						"pack.mcmeta was not valid JSON, but the game accepts it: {what}"
					)
				}));

				serde_json::from_slice(&repaired).ok()?
			}
		};

	for _ in 0..8 {
		let data = serde_json::to_vec(&value).ok()?;

		match PackMetadata::parse(pack_type, &data) {
			Ok(pack_meta) => {
				return (!fixes.is_empty()).then_some((
					pack_meta,
					MetadataFixes {
						fixed_data: Some(data),
						original_len: manifest.len(),
						fixes
					}
				));
			}
			Err(error) => fixes.extend(fix_error(&mut value, &error.to_string())?)
		}
	}

	None
}

/// Applies the repair that matches an error of the checks. Returns nothing if there is none.
fn fix_error(value: &mut Value, message: &str) -> Option<Vec<MetadataFix>> {
	let fixes = if message.contains("invalid type: floating point") {
		whole_numbers(value)
	} else if message.contains("found three or more version integers") {
		long_version_lists(value)
	} else if message.starts_with("Manifest error: on overlay") {
		Vec::new()
	} else if message.contains("`supported_formats` cannot be set when") {
		remove_supported_formats(value)
	} else if message.contains("`supported_formats` must be set when") {
		add_supported_formats(value)
	} else if message.contains("`pack_format` must be set when") {
		add_pack_format(value)
	} else {
		Vec::new()
	};

	(!fixes.is_empty()).then_some(fixes)
}

fn for_format_values(value: &mut Value, edit: &mut impl FnMut(&str, &mut Value)) {
	match value {
		Value::Object(map) => {
			for (key, child) in map.iter_mut() {
				if FORMAT_KEYS.contains(&key.as_str()) {
					edit(key, child);
				}

				for_format_values(child, edit);
			}
		}
		Value::Array(items) => items
			.iter_mut()
			.for_each(|item| for_format_values(item, edit)),
		_ => ()
	}
}

fn for_each_format_value(value: &mut Value, mut edit: impl FnMut(&str, &mut Value)) {
	for section in ["pack", "overlays"] {
		if let Some(section) = value.get_mut(section) {
			for_format_values(section, &mut edit);
		}
	}
}

fn to_whole_number(number: &mut Value) -> Option<(String, String)> {
	let float = number
		.as_f64()
		.filter(|float| number.is_f64() && float.fract() == 0.0 && float.abs() < 1e9)?;
	let old = number.to_string();

	*number = Value::from(float as i64);

	Some((old, number.to_string()))
}

fn whole_numbers(value: &mut Value) -> Vec<MetadataFix> {
	let mut fixes = Vec::new();

	for_each_format_value(value, |key, child| {
		let changed = match child {
			Value::Array(items) => items.iter_mut().filter_map(to_whole_number).collect(),
			child => to_whole_number(child).into_iter().collect::<Vec<_>>()
		};

		fixes.extend(changed.into_iter().map(|(old, new)| MetadataFix {
			summary: format!("`{key}` {old} is written as {new}"),
			detail: format!(
				"`{key}` had the decimal number {old}, which is not a valid version. It is written as the whole number {new}"
			)
		}));
	});

	fixes
}

fn long_version_lists(value: &mut Value) -> Vec<MetadataFix> {
	let mut fixes = Vec::new();

	for_each_format_value(value, |key, child| {
		if !matches!(key, "supported_formats" | "formats") {
			return;
		}

		if let Value::Array(items) = child
			&& items.len() > 2
			&& items.iter().all(Value::is_number)
		{
			let old = child.to_string();
			let range = Value::Array(vec![
				child[0].clone(),
				child[child.as_array().map_or(0, |items| items.len() - 1)].clone()
			]);
			let new = range.to_string();

			*child = range;

			fixes.push(MetadataFix {
				summary: format!("`{key}` {old} is written as {new}"),
				detail: format!(
					"`{key}` had more than two numbers, and the game only accepts a first and a last version. {old} is written as {new}, which keeps every listed version"
				)
			});
		}
	});

	fixes
}

fn pack_section(value: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
	value.get_mut("pack")?.as_object_mut()
}

fn remove_supported_formats(value: &mut Value) -> Vec<MetadataFix> {
	pack_section(value)
		.and_then(|pack| pack.remove("supported_formats"))
		.map(|old| MetadataFix {
			summary: "`supported_formats` is removed".into(),
			detail: format!(
				"`supported_formats` ({old}) is removed, because `min_format` targets game versions that reject it"
			)
		})
		.into_iter()
		.collect()
}

fn major_version(value: &Value) -> Option<i64> {
	match value {
		Value::Number(number) => number.as_i64(),
		Value::Array(items) if items.len() <= 2 && items.get(1).is_none_or(|minor| minor == 0) => {
			items.first()?.as_i64()
		}
		_ => None
	}
}

fn add_supported_formats(value: &mut Value) -> Vec<MetadataFix> {
	let Some(pack) = pack_section(value) else {
		return Vec::new();
	};
	let (Some(min), Some(max)) = (
		pack.get("min_format").and_then(major_version),
		pack.get("max_format").and_then(major_version)
	) else {
		return Vec::new();
	};
	let supported = Value::Array(vec![min.into(), max.into()]);

	pack.insert("supported_formats".into(), supported.clone());

	vec![MetadataFix {
		summary: format!("`supported_formats` {supported} is added"),
		detail: format!(
			"`min_format` targets game versions that read `supported_formats`, which was missing. {supported} is added, from `min_format` {min} and `max_format` {max}"
		)
	}]
}

fn add_pack_format(value: &mut Value) -> Vec<MetadataFix> {
	let Some(pack) = pack_section(value) else {
		return Vec::new();
	};
	let Some(min) = pack.get("min_format").and_then(major_version) else {
		return Vec::new();
	};

	pack.insert("pack_format".into(), min.into());

	vec![MetadataFix {
		summary: format!("`pack_format` {min} is added"),
		detail: format!(
			"`pack_format` was missing, and the game versions of the pack need it. {min} is added, the same as `min_format`"
		)
	}]
}
