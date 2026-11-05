//! Options that only the command-line program reads, and that are not part of the options of the
//! `packsquash` library. They are global options of the options file, like the others, but they must be
//! removed from the options file table before the rest is read as [`packsquash::config::SquashOptions`],
//! because that struct treats every unknown key as the name of a file options table.

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

/// The options that only the command-line program reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkOptions {
	/// How many directory levels are searched for a pack inside an archive.
	pub archive_pack_search_depth: usize,
	/// The most bytes that are extracted from an archive.
	pub archive_max_extracted_bytes: u64
}

impl Default for ForkOptions {
	fn default() -> Self {
		Self {
			archive_pack_search_depth: DEFAULT_MAX_PACK_DEPTH,
			archive_max_extracted_bytes: DEFAULT_MAX_EXTRACTED_BYTES
		}
	}
}

/// Tells whether the options table has any option that only the command-line program reads.
pub fn has_fork_options(table: &Table) -> bool {
	table.contains_key(ARCHIVE_PACK_SEARCH_DEPTH) || table.contains_key(ARCHIVE_MAX_EXTRACTED_MIB)
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
				archive_max_extracted_bytes: 512 * 1024 * 1024
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
	fn largest_values_do_not_overflow() {
		let mut options =
			table("archive_max_extracted_mib = 1048576\narchive_pack_search_depth = 16");

		assert_eq!(
			split_fork_options(&mut options),
			Ok(ForkOptions {
				archive_pack_search_depth: 16,
				archive_max_extracted_bytes: 1024 * 1024 * 1024 * 1024
			})
		);
	}
}
