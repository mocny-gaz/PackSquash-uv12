//! The bulk mode: squashes every pack found in a directory, whether it is a directory or an archive.
//!
//! The mode finds the packs, decides where every result goes, runs the squash operation for each pack with
//! the options of the options file, keeps going when one pack fails, and prints a report. It does not know
//! how a pack is squashed: that is a function that the caller gives, which makes the mode testable.

use std::{
	collections::HashSet,
	fmt::Write as _,
	fs,
	path::{Path, PathBuf}
};

use log::{debug, error, info, warn};
use packsquash::{
	config::SquashOptions,
	pack_archive::{ArchiveScan, ExtractLimits, extract_packs, is_pack_directory, scan_archive}
};
use toml::{Table, Value};

use crate::fork_options::BulkOptions;

/// Something that the search found in the input directory.
#[derive(Debug)]
enum Found {
	/// A directory that is a pack.
	Directory(PathBuf),
	/// An archive that has at least one pack, or that may have one if it needs a helper program.
	Archive(PathBuf, ArchiveScan),
	/// A directory or file that is not squashed, and why.
	Skipped(PathBuf, String)
}

/// How a pack ended up.
struct Outcome {
	/// A line that describes the result.
	line: String,
	failed: bool
}

/// The counts of a bulk run.
#[derive(Default)]
struct Summary {
	done: u64,
	failed: u64,
	skipped: u64,
	input_bytes: u64,
	output_bytes: u64
}

/// The function that squashes one pack with the given options.
pub type SquashFn<'a> = dyn FnMut(SquashOptions) -> Result<(), String> + 'a;

/// Removes the archive extension of a file name. `pack.tar.gz` gives `pack`.
fn archive_stem(file_name: &str) -> String {
	let Some((without_last, _)) = file_name
		.rsplit_once('.')
		.filter(|(stem, _)| !stem.is_empty())
	else {
		return file_name.to_string();
	};

	match without_last.rsplit_once('.') {
		Some((stem, tar)) if !stem.is_empty() && tar.eq_ignore_ascii_case("tar") => stem.to_string(),
		_ => without_last.to_string()
	}
}

/// Tells whether a name looks like the result of an earlier bulk run with the same prefix and suffix.
fn looks_like_output(options: &BulkOptions, stem: &str) -> bool {
	(!options.output_prefix.is_empty() || !options.output_suffix.is_empty())
		&& stem.len() >= options.output_prefix.len() + options.output_suffix.len()
		&& stem.starts_with(&options.output_prefix)
		&& stem.ends_with(&options.output_suffix)
}

/// The path of the result file for a pack, next to the input.
fn output_path(
	options: &BulkOptions,
	input: &Path,
	stem: &str,
	pack_in_archive: Option<&str>
) -> PathBuf {
	let name = match pack_in_archive {
		Some(root) => format!(
			"{}{stem}_{}{}.zip",
			options.output_prefix,
			root.replace('/', "_"),
			options.output_suffix
		),
		None => format!(
			"{}{stem}{}.zip",
			options.output_prefix, options.output_suffix
		)
	};

	input.parent().unwrap_or_else(|| Path::new("")).join(name)
}

fn file_name_of(path: &Path) -> String {
	path.file_name()
		.map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// Searches a directory tree for packs. Links are not followed. A directory that is a pack is not searched
/// inside.
fn discover(options: &BulkOptions, archive_search_depth: usize) -> Vec<Found> {
	let mut found = Vec::new();
	let mut pending = vec![(options.input_directory.clone(), 0_usize)];

	while let Some((directory, level)) = pending.pop() {
		let name = file_name_of(&directory);

		if level > 0 && looks_like_output(options, &name) {
			found.push(Found::Skipped(
				directory,
				"it looks like the result of an earlier run".into()
			));
			continue;
		}

		if is_pack_directory(&directory) {
			found.push(Found::Directory(directory));
			continue;
		}

		let mut entries = match fs::read_dir(&directory) {
			Ok(entries) => entries.filter_map(Result::ok).collect::<Vec<_>>(),
			Err(error) => {
				found.push(Found::Skipped(
					directory,
					format!("it can not be read: {error}")
				));
				continue;
			}
		};
		entries.sort_by_key(fs::DirEntry::file_name);

		let mut subdirectories = Vec::new();

		for entry in entries {
			let Ok(kind) = entry.file_type() else {
				continue;
			};
			let path = entry.path();

			if kind.is_dir() {
				if level < options.scan_depth {
					subdirectories.push((path, level + 1));
				}
			} else if kind.is_file() {
				let file_name = file_name_of(&path);
				let is_archive = path
					.extension()
					.map(|extension| extension.to_string_lossy().to_lowercase())
					.is_some_and(|extension| options.archive_extensions.contains(&extension));

				// A file in the input directory is at level 1, like a directory in it
				if !is_archive || level + 1 > options.scan_depth {
					continue;
				}

				if looks_like_output(options, &archive_stem(&file_name)) {
					found.push(Found::Skipped(
						path,
						"it looks like the result of an earlier run".into()
					));
					continue;
				}

				match scan_archive(&path, archive_search_depth) {
					Ok(Some(scan))
						if scan
							.pack_roots
							.as_ref()
							.is_none_or(|roots| !roots.is_empty()) =>
					{
						found.push(Found::Archive(path, scan));
					}
					Ok(Some(_)) => found.push(Found::Skipped(path, "it has no pack".into())),
					Ok(None) => found.push(Found::Skipped(path, "it is not an archive".into())),
					Err(error) => {
						found.push(Found::Skipped(path, format!("it can not be read: {error}")))
					}
				}
			}
		}

		// The stack gives the last pushed first, so push in reverse to visit in name order
		pending.extend(subdirectories.into_iter().rev());
	}

	found
}

/// The size in bytes of the files in a directory.
fn directory_size(directory: &Path) -> u64 {
	let mut total = 0_u64;
	let mut pending = vec![directory.to_path_buf()];

	while let Some(current) = pending.pop() {
		for entry in fs::read_dir(&current).into_iter().flatten().flatten() {
			match entry.file_type() {
				Ok(kind) if kind.is_dir() => pending.push(entry.path()),
				Ok(kind) if kind.is_file() => {
					total =
						total.saturating_add(entry.metadata().map_or(0, |metadata| metadata.len()));
				}
				_ => {}
			}
		}
	}

	total
}

fn mib(bytes: u64) -> String {
	format!("{:.2} MiB", bytes as f64 / (1024.0 * 1024.0))
}

/// Builds the options of one pack from the options of the options file.
fn options_for(table: &Table, pack_directory: &Path, output: &Path) -> Result<SquashOptions, String> {
	let mut table = table.clone();

	table.insert(
		"pack_directory".to_string(),
		Value::String(pack_directory.to_string_lossy().into_owned())
	);
	table.insert(
		"output_file_path".to_string(),
		Value::String(output.to_string_lossy().into_owned())
	);

	Value::Table(table)
		.try_into::<SquashOptions>()
		.map_err(|error| error.to_string())
}

/// What the run knows about the files around it, to refuse to write over anything that is not a result.
struct Guard {
	/// Every directory and archive that is an input.
	inputs: HashSet<PathBuf>,
	/// The result files that were already given to a pack.
	outputs: HashSet<PathBuf>
}

impl Guard {
	/// Checks that a result file can be written, and reserves it.
	fn claim(&mut self, output: &Path) -> Result<(), String> {
		if self.inputs.contains(output) {
			return Err(format!(
				"the result {} would replace a pack that is an input. Set bulk_output_prefix or \
				bulk_output_suffix, or use bulk_overwrite",
				output.display()
			));
		}

		if output.is_dir() {
			return Err(format!(
				"{} is a directory, so the result can not be written there",
				output.display()
			));
		}

		if !self.outputs.insert(output.to_path_buf()) {
			return Err(format!(
				"another pack already has the result {}. Packs with the same name in the same directory need a \
				different bulk_output_prefix or bulk_output_suffix per directory, or have to be moved",
				output.display()
			));
		}

		Ok(())
	}
}

/// How an archive with one pack replaces the original.
struct Replacement {
	/// The archive that is replaced.
	original: PathBuf,
	/// Where the original goes, if it is kept.
	backup: Option<PathBuf>,
	/// The final result file.
	result: PathBuf,
	/// The file that is written first.
	temporary: PathBuf
}

fn replacement_for(options: &BulkOptions, archive: &Path, stem: &str) -> Replacement {
	let parent = archive.parent().unwrap_or_else(|| Path::new(""));

	Replacement {
		original: archive.to_path_buf(),
		backup: options
			.backup
			.then(|| parent.join(format!("{}.original", file_name_of(archive)))),
		result: parent.join(format!("{stem}.zip")),
		temporary: parent.join(format!("{stem}.zip.squashing"))
	}
}

impl Replacement {
	/// Checks that nothing that is not the original is going to be destroyed.
	fn check(&self) -> Result<(), String> {
		if self.result != self.original && self.result.exists() {
			return Err(format!(
				"{} already exists and it is not the pack, so the pack is not replaced",
				self.result.display()
			));
		}

		if self.temporary.exists() {
			return Err(format!(
				"{} exists, probably from a run that stopped. Delete it and run again",
				self.temporary.display()
			));
		}

		if let Some(backup) = &self.backup
			&& backup.exists()
		{
			return Err(format!(
				"the backup {} already exists. Move it, or set bulk_backup to false",
				backup.display()
			));
		}

		Ok(())
	}

	/// Puts the squashed result in place of the original, after the squash operation worked.
	fn apply(&self) -> Result<(), String> {
		let fail = |what: &str, error: std::io::Error| format!("{what}: {error}");

		if let Some(backup) = &self.backup {
			fs::rename(&self.original, backup)
				.map_err(|error| fail("Couldn't keep the original pack", error))?;

			if let Err(error) = fs::rename(&self.temporary, &self.result) {
				// Put the original back, so that nothing is lost
				let _ = fs::rename(backup, &self.original);

				return Err(fail("Couldn't write the result", error));
			}

			return Ok(());
		}

		// Without backup, the result replaces the original. A rename replaces a file that exists
		fs::rename(&self.temporary, &self.result)
			.map_err(|error| fail("Couldn't write the result", error))?;

		if self.original != self.result {
			fs::remove_file(&self.original)
				.map_err(|error| fail("Couldn't delete the original pack", error))?;
		}

		Ok(())
	}
}

/// Runs the bulk mode. Returns the exit code of the program.
pub fn run(
	options: &BulkOptions,
	archive_search_depth: usize,
	extract_limits: ExtractLimits,
	table: &Table,
	squash: &mut SquashFn<'_>
) -> i32 {
	if !options.input_directory.is_dir() {
		error!(
			"bulk_input_directory {} is not a directory that exists",
			options.input_directory.display()
		);

		return 4;
	}

	info!(
		"Bulk mode{}: looking for packs in {}...",
		if options.dry_run { " (dry run)" } else { "" },
		options.input_directory.display()
	);

	let found = discover(options, archive_search_depth);
	let mut report = String::new();
	let mut summary = Summary::default();

	let mut guard = Guard {
		inputs: found
			.iter()
			.filter_map(|item| match item {
				Found::Directory(path) | Found::Archive(path, _) => Some(path.clone()),
				Found::Skipped(..) => None
			})
			.collect(),
		outputs: HashSet::new()
	};

	for item in &found {
		if let Found::Skipped(path, reason) = item {
			summary.skipped += 1;
			debug!("Skipped {}: {reason}", path.display());
			let _ = writeln!(report, "skipped {}: {reason}", path.display());
		}
	}

	let total: usize = found
		.iter()
		.map(|item| match item {
			Found::Directory(_) => 1,
			Found::Archive(_, scan) => scan.pack_roots.as_ref().map_or(1, Vec::len),
			Found::Skipped(..) => 0
		})
		.sum();

	if total == 0 {
		warn!(
			"No packs were found in {}",
			options.input_directory.display()
		);
	}

	let mut index = 0_usize;

	for item in found {
		let outcomes = match item {
			Found::Skipped(..) => continue,
			Found::Directory(path) => {
				index += 1;
				let output = output_path(options, &path, &file_name_of(&path), None);

				vec![process(
					options,
					table,
					squash,
					&mut guard,
					&path,
					&output,
					&format!("[{index}/{total}] {}", path.display()),
					directory_size(&path),
					None,
					&mut summary
				)]
			}
			Found::Archive(path, scan) => process_archive(
				options,
				extract_limits,
				archive_search_depth,
				table,
				squash,
				&mut guard,
				&path,
				&scan,
				total,
				&mut index,
				&mut summary
			)
		};

		for outcome in outcomes {
			if outcome.failed {
				error!("{}", outcome.line);
			} else {
				info!("{}", outcome.line);
			}

			let _ = writeln!(report, "{}", outcome.line);
		}
	}

	let finished = if options.dry_run {
		"would be squashed"
	} else {
		"squashed"
	};
	let summary_text = format!(
		"Bulk mode finished: {} {finished}, {} failed, {} skipped. {}",
		summary.done,
		summary.failed,
		summary.skipped,
		if options.dry_run {
			String::new()
		} else {
			format!(
				"Size: {} -> {}.",
				mib(summary.input_bytes),
				mib(summary.output_bytes)
			)
		}
	);

	if summary.failed > 0 {
		error!("{summary_text}");
		error!(
			"More help for this fork: <{}>",
			crate::error_hints::FORK_TROUBLESHOOTING_URL
		);
	} else {
		info!("{summary_text}");
	}

	let _ = writeln!(report, "{summary_text}");

	if let Some(report_file) = &options.report_file {
		match fs::write(report_file, report) {
			Ok(()) => info!("Report written to {}", report_file.display()),
			Err(error) => error!(
				"Couldn't write the report to {}: {error}",
				report_file.display()
			)
		}
	}

	i32::from(summary.failed > 0)
}

/// Squashes one pack, or tells what would happen in a dry run. `replacement` is set for an archive that is replaced.
#[allow(clippy::too_many_arguments)]
fn process(
	options: &BulkOptions,
	table: &Table,
	squash: &mut SquashFn<'_>,
	guard: &mut Guard,
	pack_directory: &Path,
	output: &Path,
	label: &str,
	input_bytes: u64,
	replacement: Option<&Replacement>,
	summary: &mut Summary
) -> Outcome {
	let fail = |summary: &mut Summary, message: String| {
		summary.failed += 1;

		Outcome {
			line: format!("{label}: failed: {message}"),
			failed: true
		}
	};

	let final_output = replacement.map_or(output, |replacement| replacement.result.as_path());

	// An archive that is replaced writes to a temporary file first, and its result path is its original path
	let written_path = replacement.map_or(output, |replacement| replacement.temporary.as_path());

	if replacement.is_none() {
		if let Err(message) = guard.claim(output) {
			return fail(summary, message);
		}
	} else if !guard.outputs.insert(final_output.to_path_buf()) {
		return fail(
			summary,
			format!(
				"another pack already has the result {}",
				final_output.display()
			)
		);
	}

	if let Some(replacement) = replacement
		&& let Err(message) = replacement.check()
	{
		return fail(summary, message);
	}

	if options.dry_run {
		summary.done += 1;

		return Outcome {
			line: format!(
				"{label} -> {}{}",
				final_output.display(),
				match replacement {
					Some(replacement) => match &replacement.backup {
						Some(backup) =>
							format!(" (the original would be kept as {})", backup.display()),
						None => " (the original would be deleted)".to_string()
					},
					None => String::new()
				}
			),
			failed: false
		};
	}

	let squash_options = match options_for(table, pack_directory, written_path) {
		Ok(squash_options) => squash_options,
		Err(message) => {
			return fail(
				summary,
				format!("the options are not valid for this pack: {message}")
			);
		}
	};

	if let Err(message) = squash(squash_options) {
		if replacement.is_some() {
			let _ = fs::remove_file(written_path);
		}

		return fail(summary, message);
	}

	if let Some(replacement) = replacement
		&& let Err(message) = replacement.apply()
	{
		let _ = fs::remove_file(&replacement.temporary);

		return fail(summary, message);
	}

	let output_bytes = fs::metadata(final_output).map_or(0, |metadata| metadata.len());

	summary.done += 1;
	summary.input_bytes = summary.input_bytes.saturating_add(input_bytes);
	summary.output_bytes = summary.output_bytes.saturating_add(output_bytes);

	Outcome {
		line: format!(
			"{label} -> {} ({} -> {})",
			final_output.display(),
			mib(input_bytes),
			mib(output_bytes)
		),
		failed: false
	}
}

/// Squashes the packs of one archive. The packs are extracted together, once, and the extracted files are deleted
/// when this function returns.
#[allow(clippy::too_many_arguments)]
fn process_archive(
	options: &BulkOptions,
	extract_limits: ExtractLimits,
	archive_search_depth: usize,
	table: &Table,
	squash: &mut SquashFn<'_>,
	guard: &mut Guard,
	archive: &Path,
	scan: &ArchiveScan,
	total: usize,
	index: &mut usize,
	summary: &mut Summary
) -> Vec<Outcome> {
	let stem = archive_stem(&file_name_of(archive));
	let archive_bytes = fs::metadata(archive).map_or(0, |metadata| metadata.len());

	let failure = |summary: &mut Summary, index: usize, message: String| {
		summary.failed += 1;

		Outcome {
			line: format!("[{index}/{total}] {}: failed: {message}", archive.display()),
			failed: true
		}
	};

	// In a dry run, nothing is extracted, so the packs are the ones that the scan found
	if options.dry_run {
		let roots = scan
			.pack_roots
			.clone()
			.unwrap_or_else(|| vec![String::new()]);
		let several = roots.len() > 1;
		let mut outcomes = Vec::new();

		for root in &roots {
			*index += 1;
			let label = format!(
				"[{index}/{total}] {}{}",
				archive.display(),
				if root.is_empty() {
					String::new()
				} else {
					format!(" ({root})")
				}
			);
			let replacement =
				(options.overwrite && !several).then(|| replacement_for(options, archive, &stem));
			let output = output_path(options, archive, &stem, several.then_some(root.as_str()));

			outcomes.push(process(
				options,
				table,
				squash,
				guard,
				archive,
				&output,
				&label,
				archive_bytes,
				replacement.as_ref(),
				summary
			));
		}

		return outcomes;
	}

	let extracted = match extract_packs(archive, scan, archive_search_depth, extract_limits) {
		Ok(extracted) => extracted,
		Err(error) => {
			*index += 1;

			return vec![failure(summary, *index, format!("{error}"))];
		}
	};

	// The scan of an archive that needs a helper program does not know the number of packs. If the real number
	// is different from the one that was counted, the numbers in the labels are not exact, but the work is right
	let several = extracted.packs.len() > 1;
	let mut outcomes = Vec::new();

	for (root, pack_directory) in &extracted.packs {
		*index += 1;

		let label = format!(
			"[{index}/{total}] {}{}",
			archive.display(),
			if root.is_empty() {
				String::new()
			} else {
				format!(" ({root})")
			}
		);
		let replacement =
			(options.overwrite && !several).then(|| replacement_for(options, archive, &stem));
		let output = output_path(options, archive, &stem, several.then_some(root.as_str()));

		outcomes.push(process(
			options,
			table,
			squash,
			guard,
			pack_directory,
			&output,
			&label,
			archive_bytes,
			replacement.as_ref(),
			summary
		));
	}

	outcomes
}

#[cfg(test)]
mod tests;
