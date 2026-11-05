//! Options that only the command-line program reads, and that are not part of the options of the
//! `packsquash` library. They are global options of the options file, like the others, but they must be
//! removed from the options file table before the rest is read as [`packsquash::config::SquashOptions`],
//! because that struct treats every unknown key as the name of a file options table.

use std::path::PathBuf;

use packsquash::pack_archive::{DEFAULT_MAX_EXTRACTED_BYTES, DEFAULT_MAX_PACK_DEPTH};
use toml::{Table, Value};

/// The name of the option that sets how many directory levels are searched for a pack in an archive.
const ARCHIVE_PACK_SEARCH_DEPTH: &str = "archive_pack_search_depth";
/// The name of the option that limits how much data is extracted from an archive.
const ARCHIVE_MAX_EXTRACTED_MIB: &str = "archive_max_extracted_mib";

/// The most directory levels that `archive_pack_search_depth` accepts.
const MAX_SEARCH_DEPTH: i64 = 16;
/// The most MiB that `archive_max_extracted_mib` accepts.
const MAX_EXTRACTED_MIB: i64 = 1_048_576;

/// The name of the option that enables the bulk mode.
const BULK_INPUT_DIRECTORY: &str = "bulk_input_directory";
const BULK_OUTPUT_PREFIX: &str = "bulk_output_prefix";
const BULK_OUTPUT_SUFFIX: &str = "bulk_output_suffix";
const BULK_OVERWRITE: &str = "bulk_overwrite";
const BULK_BACKUP: &str = "bulk_backup";
const BULK_DRY_RUN: &str = "bulk_dry_run";
const BULK_REPORT_FILE: &str = "bulk_report_file";
const BULK_ARCHIVE_EXTENSIONS: &str = "bulk_archive_extensions";
const BULK_SCAN_DEPTH: &str = "bulk_scan_depth";

/// All the options of the bulk mode.
const BULK_OPTIONS: [&str; 9] = [
	BULK_INPUT_DIRECTORY,
	BULK_OUTPUT_PREFIX,
	BULK_OUTPUT_SUFFIX,
	BULK_OVERWRITE,
	BULK_BACKUP,
	BULK_DRY_RUN,
	BULK_REPORT_FILE,
	BULK_ARCHIVE_EXTENSIONS,
	BULK_SCAN_DEPTH
];

/// The file extensions that the bulk mode opens as archives by default. `jar` is not in the list on
/// purpose, because the jar files of mods also have a `pack.mcmeta` file and an `assets` directory.
const DEFAULT_BULK_ARCHIVE_EXTENSIONS: &str = "zip,7z,rar,tar,gz,tgz,xz,txz,zst,tzst,bz2,tbz2";

/// The most directory levels that `bulk_scan_depth` accepts.
const MAX_BULK_SCAN_DEPTH: i64 = 256;

/// The options of the bulk mode, which squashes every pack found in a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkOptions {
	/// The directory that is searched for packs.
	pub input_directory: PathBuf,
	/// The text that every output file name starts with.
	pub output_prefix: String,
	/// The text that every output file name ends with, before `.zip`.
	pub output_suffix: String,
	/// Whether the squashed pack replaces the original pack.
	pub overwrite: bool,
	/// Whether the original pack is kept when it is replaced.
	pub backup: bool,
	/// Whether to only tell what would happen.
	pub dry_run: bool,
	/// A file where the report is also written.
	pub report_file: Option<PathBuf>,
	/// The file extensions that are opened as archives, in lowercase and without a dot.
	pub archive_extensions: Vec<String>,
	/// How many directory levels are searched.
	pub scan_depth: usize
}

/// The options that only the command-line program reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkOptions {
	/// How many directory levels are searched for a pack inside an archive.
	pub archive_pack_search_depth: usize,
	/// The most bytes that are extracted from an archive.
	pub archive_max_extracted_bytes: u64,
	/// The options of the bulk mode, if it is enabled.
	pub bulk: Option<BulkOptions>
}

impl Default for ForkOptions {
	fn default() -> Self {
		Self {
			archive_pack_search_depth: DEFAULT_MAX_PACK_DEPTH,
			archive_max_extracted_bytes: DEFAULT_MAX_EXTRACTED_BYTES,
			bulk: None
		}
	}
}

/// Tells whether the options table has any option that only the command-line program reads.
pub fn has_fork_options(table: &Table) -> bool {
	table.contains_key(ARCHIVE_PACK_SEARCH_DEPTH)
		|| table.contains_key(ARCHIVE_MAX_EXTRACTED_MIB)
		|| BULK_OPTIONS.iter().any(|name| table.contains_key(*name))
}

/// Reads one whole number option with a range, and removes it from the table.
fn take_integer(table: &mut Table, name: &str, maximum: i64) -> Result<Option<i64>, String> {
	match table.remove(name) {
		None => Ok(None),
		Some(Value::Integer(value)) if (1..=maximum).contains(&value) => Ok(Some(value)),
		Some(Value::Integer(value)) => Err(format!(
			"The option {name} is {value}, but it must be a whole number from 1 to {maximum}"
		)),
		Some(other) => Err(format!(
			"The option {name} must be a whole number from 1 to {maximum}, but it is a {}",
			other.type_str()
		))
	}
}

/// Reads one text option and removes it from the table.
fn take_string(table: &mut Table, name: &str) -> Result<Option<String>, String> {
	match table.remove(name) {
		None => Ok(None),
		Some(Value::String(value)) => Ok(Some(value)),
		Some(other) => Err(format!(
			"The option {name} must be a text in quotes, but it is a {}",
			other.type_str()
		))
	}
}

/// Reads one true or false option and removes it from the table.
fn take_bool(table: &mut Table, name: &str) -> Result<Option<bool>, String> {
	match table.remove(name) {
		None => Ok(None),
		Some(Value::Boolean(value)) => Ok(Some(value)),
		Some(other) => Err(format!(
			"The option {name} must be true or false, but it is a {}",
			other.type_str()
		))
	}
}

/// Checks that a text is fine to be a part of a file name.
fn check_file_name_part(name: &str, value: &str) -> Result<(), String> {
	match value
		.chars()
		.find(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
	{
		Some(c) => Err(format!(
			"The option {name} can not have the character {c:?}, because it is part of a file name"
		)),
		None => Ok(())
	}
}

/// Splits the list of archive file extensions of the bulk mode.
fn parse_archive_extensions(value: &str) -> Result<Vec<String>, String> {
	let mut extensions: Vec<String> = Vec::new();

	for extension in value
		.split(',')
		.map(|extension| extension.trim().trim_start_matches('.').to_lowercase())
		.filter(|extension| !extension.is_empty())
	{
		if extension.contains(['/', '\\', '.']) {
			return Err(format!(
				"The option {BULK_ARCHIVE_EXTENSIONS} has the extension {extension:?}. Write only the last part of the \
				file name, like tgz, separated by commas"
			));
		}

		if !extensions.contains(&extension) {
			extensions.push(extension);
		}
	}

	if extensions.is_empty() {
		Err(format!(
			"The option {BULK_ARCHIVE_EXTENSIONS} has no extension"
		))
	} else {
		Ok(extensions)
	}
}

/// Removes the options of the bulk mode from the table, and returns them if the mode is enabled.
fn take_bulk_options(table: &mut Table) -> Result<Option<BulkOptions>, String> {
	let input_directory = take_string(table, BULK_INPUT_DIRECTORY)?;
	let prefix = take_string(table, BULK_OUTPUT_PREFIX)?;
	let suffix = take_string(table, BULK_OUTPUT_SUFFIX)?;
	let overwrite = take_bool(table, BULK_OVERWRITE)?;
	let backup = take_bool(table, BULK_BACKUP)?;
	let dry_run = take_bool(table, BULK_DRY_RUN)?;
	let report_file = take_string(table, BULK_REPORT_FILE)?;
	let extensions = take_string(table, BULK_ARCHIVE_EXTENSIONS)?;
	let scan_depth = take_integer(table, BULK_SCAN_DEPTH, MAX_BULK_SCAN_DEPTH)?;

	let Some(input_directory) = input_directory else {
		let others_set = [
			("bulk_output_prefix", prefix.is_some()),
			("bulk_output_suffix", suffix.is_some()),
			("bulk_overwrite", overwrite.is_some()),
			("bulk_backup", backup.is_some()),
			("bulk_dry_run", dry_run.is_some()),
			("bulk_report_file", report_file.is_some()),
			("bulk_archive_extensions", extensions.is_some()),
			("bulk_scan_depth", scan_depth.is_some())
		];

		return match others_set.iter().find(|(_, is_set)| *is_set) {
			Some((name, _)) => Err(format!(
				"The option {name} is set, but the bulk mode is off. Set bulk_input_directory to the directory with the packs to turn it on"
			)),
			None => Ok(None)
		};
	};

	if input_directory.trim().is_empty() {
		return Err(format!("The option {BULK_INPUT_DIRECTORY} is empty"));
	}

	let prefix = prefix.unwrap_or_else(|| "squashed_".to_string());
	let suffix = suffix.unwrap_or_default();
	check_file_name_part(BULK_OUTPUT_PREFIX, &prefix)?;
	check_file_name_part(BULK_OUTPUT_SUFFIX, &suffix)?;

	if report_file
		.as_deref()
		.is_some_and(|path| path.trim().is_empty())
	{
		return Err(format!("The option {BULK_REPORT_FILE} is empty"));
	}

	Ok(Some(BulkOptions {
		input_directory: PathBuf::from(input_directory),
		output_prefix: prefix,
		output_suffix: suffix,
		overwrite: overwrite.unwrap_or(false),
		backup: backup.unwrap_or(true),
		dry_run: dry_run.unwrap_or(false),
		report_file: report_file.map(PathBuf::from),
		archive_extensions: parse_archive_extensions(
			extensions
				.as_deref()
				.unwrap_or(DEFAULT_BULK_ARCHIVE_EXTENSIONS)
		)?,
		scan_depth: scan_depth.map_or(32, |depth| depth as usize)
	}))
}

/// Removes the options that only the command-line program reads from the options table, and returns them.
/// Options that are not in the table have their default value.
pub fn split_fork_options(table: &mut Table) -> Result<ForkOptions, String> {
	let mut options = ForkOptions::default();

	if let Some(depth) = take_integer(table, ARCHIVE_PACK_SEARCH_DEPTH, MAX_SEARCH_DEPTH)? {
		options.archive_pack_search_depth = depth as usize;
	}

	if let Some(mebibytes) = take_integer(table, ARCHIVE_MAX_EXTRACTED_MIB, MAX_EXTRACTED_MIB)? {
		options.archive_max_extracted_bytes = mebibytes as u64 * 1024 * 1024;
	}

	options.bulk = take_bulk_options(table)?;

	Ok(options)
}

#[cfg(test)]
mod tests {
	use super::*;

	fn table(text: &str) -> Table {
		text.parse().expect("the test options are valid TOML")
	}

	#[test]
	fn options_that_are_not_set_have_their_default() {
		let mut options = table("pack_directory = 'pack'\n");

		assert!(!has_fork_options(&options));
		assert_eq!(split_fork_options(&mut options), Ok(ForkOptions::default()));
		assert_eq!(options.len(), 1, "other options stay in the table");
	}

	#[test]
	fn options_are_read_and_removed_from_the_table() {
		let mut options = table(
			"pack_directory = 'pack.7z'\narchive_pack_search_depth = 2\narchive_max_extracted_mib = 512\nzip_comment = 'a'\n"
		);

		assert!(has_fork_options(&options));
		assert_eq!(
			split_fork_options(&mut options),
			Ok(ForkOptions {
				archive_pack_search_depth: 2,
				archive_max_extracted_bytes: 512 * 1024 * 1024,
				bulk: None
			})
		);
		assert!(!has_fork_options(&options));
		assert!(options.contains_key("pack_directory") && options.contains_key("zip_comment"));
		assert_eq!(options.len(), 2);
	}

	#[test]
	fn values_out_of_range_are_rejected() {
		for text in [
			"archive_pack_search_depth = 0",
			"archive_pack_search_depth = 17",
			"archive_pack_search_depth = -1",
			"archive_max_extracted_mib = 0",
			"archive_max_extracted_mib = 1048577"
		] {
			let error = split_fork_options(&mut table(text)).expect_err(text);
			assert!(
				error.contains("must be a whole number from 1 to"),
				"{error}"
			);
		}
	}

	#[test]
	fn values_of_a_wrong_type_are_rejected() {
		for text in [
			"archive_pack_search_depth = '4'",
			"archive_pack_search_depth = 1.5",
			"archive_max_extracted_mib = true"
		] {
			let error = split_fork_options(&mut table(text)).expect_err(text);
			assert!(error.contains("but it is a"), "{error}");
		}
	}

	#[test]
	fn bulk_mode_is_off_without_its_directory() {
		let mut options = table("pack_directory = 'pack'\n");

		assert_eq!(split_fork_options(&mut options).unwrap().bulk, None);
	}

	#[test]
	fn bulk_mode_has_defaults() {
		let mut options = table("bulk_input_directory = 'C:\\packs'\n");

		assert!(has_fork_options(&options));

		let bulk = split_fork_options(&mut options).unwrap().bulk.unwrap();

		assert_eq!(bulk.input_directory, PathBuf::from("C:\\packs"));
		assert_eq!(bulk.output_prefix, "squashed_");
		assert_eq!(bulk.output_suffix, "");
		assert!(!bulk.overwrite && bulk.backup && !bulk.dry_run);
		assert_eq!(bulk.report_file, None);
		assert_eq!(bulk.scan_depth, 32);
		assert!(bulk.archive_extensions.contains(&"7z".to_string()));
		assert!(!bulk.archive_extensions.contains(&"jar".to_string()));
		assert!(
			options.is_empty(),
			"every bulk option is removed from the table"
		);
	}

	#[test]
	fn bulk_options_are_read() {
		let mut options = table(
			"bulk_input_directory = 'in'\nbulk_output_prefix = ''\nbulk_output_suffix = '_small'\n\
			bulk_overwrite = true\nbulk_backup = false\nbulk_dry_run = true\nbulk_report_file = 'r.txt'\n\
			bulk_archive_extensions = ' .ZIP, tgz,,zip '\nbulk_scan_depth = 3\n"
		);
		let bulk = split_fork_options(&mut options).unwrap().bulk.unwrap();

		assert_eq!(bulk.output_prefix, "");
		assert_eq!(bulk.output_suffix, "_small");
		assert!(bulk.overwrite && !bulk.backup && bulk.dry_run);
		assert_eq!(bulk.report_file, Some(PathBuf::from("r.txt")));
		assert_eq!(bulk.archive_extensions, ["zip", "tgz"]);
		assert_eq!(bulk.scan_depth, 3);
	}

	#[test]
	fn bulk_options_without_the_directory_are_an_error() {
		let error = split_fork_options(&mut table("bulk_dry_run = true")).unwrap_err();

		assert!(
			error.contains("bulk_dry_run") && error.contains("bulk_input_directory"),
			"{error}"
		);
	}

	#[test]
	fn bad_bulk_values_are_rejected() {
		for text in [
			"bulk_input_directory = ''",
			"bulk_input_directory = 3",
			"bulk_input_directory = 'a'\nbulk_overwrite = 'yes'",
			"bulk_input_directory = 'a'\nbulk_output_prefix = 'x/y'",
			"bulk_input_directory = 'a'\nbulk_output_suffix = 'a:b'",
			"bulk_input_directory = 'a'\nbulk_archive_extensions = ''",
			"bulk_input_directory = 'a'\nbulk_archive_extensions = 'tar.gz'",
			"bulk_input_directory = 'a'\nbulk_scan_depth = 0",
			"bulk_input_directory = 'a'\nbulk_scan_depth = 257",
			"bulk_input_directory = 'a'\nbulk_report_file = ' '"
		] {
			assert!(split_fork_options(&mut table(text)).is_err(), "{text}");
		}
	}

	#[test]
	fn largest_values_do_not_overflow() {
		let mut options =
			table("archive_max_extracted_mib = 1048576\narchive_pack_search_depth = 16");

		assert_eq!(
			split_fork_options(&mut options),
			Ok(ForkOptions {
				archive_pack_search_depth: 16,
				archive_max_extracted_bytes: 1024 * 1024 * 1024 * 1024,
				bulk: None
			})
		);
	}
}
