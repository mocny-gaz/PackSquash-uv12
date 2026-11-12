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
	pack_archive::{
		ArchiveFormat, ArchiveScan, ExtractLimits, NestedArchive, extract_nested_archives,
		extract_packs_and_nested, is_pack_directory, scan_archive_with_nested
	}
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

/// What happened to a pack or an archive.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
	/// It was squashed, or it would be in a dry run.
	Done,
	/// Squashing it failed.
	Failed,
	/// It is not squashed, and why is only in the report and the debug log.
	Skipped,
	/// Something that the user should read, but that is not counted.
	Note
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
pub(crate) fn archive_stem(file_name: &str) -> String {
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

/// How the results are named and where they go.
struct Layout {
	/// The text that every result name starts with.
	prefix: String,
	/// The text that every result name ends with, before `.zip`.
	suffix: String,
	/// The directory that the folders of the input are counted from. For an archive that is the input by itself,
	/// it is the directory that holds the archive.
	input_root: PathBuf,
	/// Where the results go, when it is not next to the input.
	output_root: Option<PathBuf>
}

/// Tells whether two paths are the same directory, even when they are written in different ways. A directory that
/// does not exist yet is the same only when it is written in the same way.
fn same_directory(first: &Path, second: &Path) -> bool {
	match (fs::canonicalize(first), fs::canonicalize(second)) {
		(Ok(first), Ok(second)) => first == second,
		_ => first == second
	}
}

impl Layout {
	fn new(options: &BulkOptions) -> Self {
		let input = &options.input_directory;
		let input_root = if input.is_file() {
			input.parent().map(Path::to_path_buf).unwrap_or_default()
		} else {
			input.clone()
		};

		// An output directory that is the input directory is the same as no output directory
		let output_root = options
			.output_directory
			.clone()
			.filter(|output| !same_directory(output, &input_root));

		// The prefix is on by default only for the results that are next to the input
		let prefix = options.output_prefix.clone().unwrap_or_else(|| {
			if output_root.is_some() {
				String::new()
			} else {
				"squashed_".to_string()
			}
		});

		Self {
			prefix,
			suffix: options.output_suffix.clone(),
			input_root,
			output_root
		}
	}

	/// Tells whether a name looks like the result of an earlier bulk run with the same prefix and suffix.
	fn looks_like_output(&self, stem: &str) -> bool {
		(!self.prefix.is_empty() || !self.suffix.is_empty())
			&& stem.len() >= self.prefix.len() + self.suffix.len()
			&& stem.starts_with(&self.prefix)
			&& stem.ends_with(&self.suffix)
	}

	/// The directory where the results of what is in `input_parent` go.
	fn result_directory(&self, input_parent: &Path) -> PathBuf {
		match &self.output_root {
			Some(output) => output.join(
				input_parent
					.strip_prefix(&self.input_root)
					.unwrap_or_else(|_| Path::new(""))
			),
			None => input_parent.to_path_buf()
		}
	}

	/// The name of the result of something in the input, with the prefix and the suffix.
	fn result_name(&self, stem: &str) -> String {
		format!("{}{stem}{}", self.prefix, self.suffix)
	}
}

/// Where the results of an archive go. An archive with one pack and nothing else gives the file
/// `directory/name.zip`. Any other archive gives the folder `directory/name`, which has inside what the archive
/// has: a pack in a directory of the archive is a zip file in the same directory of the folder, and an archive inside
/// the archive is handled like this one, with its own name.
#[derive(Debug, Clone)]
struct Dest {
	directory: PathBuf,
	/// The name without `.zip`.
	name: String
}

impl Dest {
	fn file(&self) -> PathBuf {
		self.directory.join(format!("{}.zip", self.name))
	}

	fn folder(&self) -> PathBuf {
		self.directory.join(&self.name)
	}

	/// The result file of the pack that is in the given directory of the archive, when the archive gives a folder.
	fn pack_file(&self, root: &str) -> PathBuf {
		match root.rsplit_once('/') {
			Some((parent, last)) => self
				.folder()
				.join(parent.split('/').collect::<PathBuf>())
				.join(format!("{last}.zip")),
			None if root.is_empty() => self.folder().join(format!("{}.zip", self.name)),
			None => self.folder().join(format!("{root}.zip"))
		}
	}

	/// Where the results of an archive that is inside this one, at the given path of the archive, go.
	fn nested(&self, name: &str) -> Self {
		let (parent, file) = name.rsplit_once('/').unwrap_or(("", name));
		let mut directory = self.folder();

		if !parent.is_empty() {
			directory = directory.join(parent.split('/').collect::<PathBuf>());
		}

		Self {
			directory,
			name: archive_stem(file)
		}
	}
}

/// The most archives that can be inside each other, whatever the options say. It keeps the disk space that the
/// extracted archives need, and the depth of the calls, bounded. A deeper archive is skipped.
const MAX_NESTING: usize = 32;

/// How many archives can be inside each other: the same number as the levels that the search goes down, which is
/// `bulk_scan_depth` (or `archive_pack_search_depth`, if it is higher), and at most [`MAX_NESTING`].
fn nesting_limit(search_depth: usize) -> usize {
	search_depth.min(MAX_NESTING)
}

/// The most bytes of nested archives that are extracted together from a ZIP archive, which can give one file
/// without reading the others. Archives of other formats have to be read from the start for every extraction, so
/// all of the nested archives are extracted at once.
const NESTED_ZIP_BATCH_BYTES: u64 = 2 * 1024 * 1024 * 1024;

/// Groups the names of nested archives, so that every group is extracted at once.
fn nested_batches(nested: &[NestedArchive], format: ArchiveFormat) -> Vec<Vec<String>> {
	let limit = if format == ArchiveFormat::Zip {
		NESTED_ZIP_BATCH_BYTES
	} else {
		u64::MAX
	};
	let mut batches = Vec::new();
	let mut current: Vec<String> = Vec::new();
	let mut size = 0_u64;

	for archive in nested {
		if !current.is_empty() && size.saturating_add(archive.size) > limit {
			batches.push(std::mem::take(&mut current));
			size = 0;
		}

		current.push(archive.name.clone());
		size = size.saturating_add(archive.size);
	}

	if !current.is_empty() {
		batches.push(current);
	}

	batches
}

fn file_name_of(path: &Path) -> String {
	path.file_name()
		.map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// Tells whether an archive has something to squash: a pack, an archive inside it that may have packs, or
/// contents that are only known after a helper program extracts them.
fn has_content(scan: &ArchiveScan) -> bool {
	scan.pack_roots
		.as_ref()
		.is_none_or(|roots| !roots.is_empty())
		|| !scan.nested_archives.is_empty()
}

/// Searches a directory tree for packs. Links are not followed. A directory that is a pack is not searched
/// inside.
fn discover(options: &BulkOptions, layout: &Layout, archive_search_depth: usize) -> Vec<Found> {
	let mut found = Vec::new();
	let mut pending = vec![(options.input_directory.clone(), 0_usize)];

	while let Some((directory, level)) = pending.pop() {
		let name = file_name_of(&directory);

		if level > 0 && layout.looks_like_output(&name) {
			found.push(Found::Skipped(
				directory,
				"it looks like the result of an earlier run".into()
			));
			continue;
		}

		// The output directory can be inside the input directory. What is in it is a result, not an input
		if level > 0
			&& layout
				.output_root
				.as_ref()
				.is_some_and(|output| same_directory(output, &directory))
		{
			found.push(Found::Skipped(
				directory,
				"it is the output directory".into()
			));
			continue;
		}

		if is_pack_directory(&directory) {
			debug!(
				target: crate::debug_log::DEBUG_LOG_TARGET,
				"Bulk search: {} is a pack directory (level {level}), so it is not searched inside",
				directory.display()
			);

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
				} else {
					debug!(
						target: crate::debug_log::DEBUG_LOG_TARGET,
						"Bulk search: {} is not searched, because it is deeper than bulk_scan_depth ({})",
						path.display(),
						options.scan_depth
					);
				}
			} else if kind.is_file() {
				let file_name = file_name_of(&path);
				let is_archive = path
					.extension()
					.map(|extension| extension.to_string_lossy().to_lowercase())
					.is_some_and(|extension| options.archive_extensions.contains(&extension));

				// A file in the input directory is at level 1, like a directory in it
				if !is_archive || level + 1 > options.scan_depth {
					if is_archive {
						debug!(
							target: crate::debug_log::DEBUG_LOG_TARGET,
							"Bulk search: the archive {} is not opened, because it is deeper than bulk_scan_depth ({})",
							path.display(),
							options.scan_depth
						);
					}

					continue;
				}

				if layout.looks_like_output(&archive_stem(&file_name)) {
					found.push(Found::Skipped(
						path,
						"it looks like the result of an earlier run".into()
					));
					continue;
				}

				match scan_archive_with_nested(
					&path,
					archive_search_depth,
					&options.archive_extensions
				) {
					Ok(Some(scan)) if has_content(&scan) => {
						found.push(Found::Archive(path, scan));
					}
					Ok(Some(_)) => found.push(Found::Skipped(path, "it has no pack".into())),
					Ok(None) => found.push(Found::Skipped(path, "it is not an archive".into())),
					Err(error) => {
						debug!(
							target: crate::debug_log::DEBUG_LOG_TARGET,
							"The archive {} could not be read. Error, with the errors that caused it: {}",
							path.display(),
							crate::debug_log::error_chain(&error)
						);

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

/// Reads an archive file that is the input of the bulk mode by itself. Its name and extension do not matter, and
/// its name is not checked for being the result of an earlier run, because the user chose it.
fn discover_archive_file(
	path: &Path,
	archive_search_depth: usize,
	archive_extensions: &[String]
) -> Result<Vec<Found>, String> {
	match scan_archive_with_nested(path, archive_search_depth, archive_extensions) {
		Ok(Some(scan)) if has_content(&scan) => Ok(vec![Found::Archive(path.to_path_buf(), scan)]),
		Ok(Some(_)) => Ok(vec![Found::Skipped(
			path.to_path_buf(),
			"it has no pack".into()
		)]),
		Ok(None) => Err(format!(
			"bulk_input_directory {} is a file, but not an archive that PackSquash can read. It can be a \
			directory with packs, or a ZIP, 7z, TAR (also .tar.gz, .tar.xz, .tar.zst and .tar.bz2) or RAR archive",
			path.display()
		)),
		Err(error) => {
			debug!(
				target: crate::debug_log::DEBUG_LOG_TARGET,
				"The archive {} could not be read. Error, with the errors that caused it: {}",
				path.display(),
				crate::debug_log::error_chain(&error)
			);

			Err(format!(
				"Couldn't read the archive {}: {error}",
				path.display()
			))
		}
	}
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

/// What the run needs to know and does not change.
struct Env<'a> {
	options: &'a BulkOptions,
	layout: &'a Layout,
	extract_limits: ExtractLimits,
	/// How many directory levels are searched for packs, in directories and in archives.
	search_depth: usize,
	table: &'a Table
}

/// What the run changes: the files it has promised, the counts, and the report.
struct State {
	guard: Guard,
	summary: Summary,
	report: String,
	/// How many packs were numbered.
	index: usize,
	/// How many packs the search found. It is exact only when `exact` is set.
	total: usize,
	/// False when an archive has archives inside it, or needs a helper program: the number of packs is only
	/// known when they are opened.
	exact: bool
}

impl State {
	/// Tells about something right away, so that a long run shows its progress, and keeps it for the report.
	fn emit(&mut self, kind: Kind, line: String) {
		match kind {
			Kind::Done | Kind::Note => info!("{line}"),
			Kind::Failed => {
				self.summary.failed += 1;
				error!("{line}");
			}
			Kind::Skipped => {
				self.summary.skipped += 1;
				debug!("{line}");
			}
		}

		let _ = writeln!(self.report, "{line}");
	}

	fn fail(&mut self, label: &str, message: &str) {
		self.emit(Kind::Failed, format!("{label}: failed: {message}"));
	}

	/// Numbers the next pack and makes its label: its number, its name and where it is.
	fn label(&mut self, name: &str, source: &str) -> String {
		self.index += 1;

		if self.exact {
			format!("[{}/{}] {name} ({source})", self.index, self.total)
		} else {
			format!("[{}] {name} ({source})", self.index)
		}
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
	info!(
		"Bulk mode{}: looking for packs in {}...",
		if options.dry_run { " (dry run)" } else { "" },
		options.input_directory.display()
	);

	let layout = Layout::new(options);

	if let Some(output) = &layout.output_root {
		info!("The results go to {}", output.display());

		if options.overwrite {
			warn!(
				"bulk_overwrite is ignored, because bulk_output_directory is set. The originals are never replaced when the results go to another directory"
			);
		}
	}

	// The input is usually a directory, but it can also be a single archive file, of any format
	let found = if options.input_directory.is_dir() {
		discover(options, &layout, archive_search_depth)
	} else if options.input_directory.is_file() {
		match discover_archive_file(
			&options.input_directory,
			archive_search_depth,
			&options.archive_extensions
		) {
			Ok(found) => found,
			Err(message) => {
				error!("{message}");

				return 4;
			}
		}
	} else {
		error!(
			"bulk_input_directory {} does not exist. It can be a directory with packs, or an archive",
			options.input_directory.display()
		);

		return 4;
	};
	debug!(
		target: crate::debug_log::DEBUG_LOG_TARGET,
		"Bulk search done: {} packs or archives with packs, {} skipped. Directory levels searched: {} (bulk_scan_depth), levels searched inside archives: {archive_search_depth}",
		found.iter().filter(|item| !matches!(item, Found::Skipped(..))).count(),
		found.iter().filter(|item| matches!(item, Found::Skipped(..))).count(),
		options.scan_depth
	);

	let total: usize = found
		.iter()
		.map(|item| match item {
			Found::Directory(_) => 1,
			Found::Archive(_, scan) => scan.pack_roots.as_ref().map_or(1, Vec::len),
			Found::Skipped(..) => 0
		})
		.sum();
	let exact = found.iter().all(|item| match item {
		Found::Archive(_, scan) => scan.pack_roots.is_some() && scan.nested_archives.is_empty(),
		_ => true
	});

	let mut state = State {
		guard: Guard {
			inputs: found
				.iter()
				.filter_map(|item| match item {
					Found::Directory(path) | Found::Archive(path, _) => Some(path.clone()),
					Found::Skipped(..) => None
				})
				.collect(),
			outputs: HashSet::new()
		},
		summary: Summary::default(),
		report: String::new(),
		index: 0,
		total,
		exact
	};

	for item in &found {
		if let Found::Skipped(path, reason) = item {
			state.emit(
				Kind::Skipped,
				format!("skipped {}: {reason}", path.display())
			);
		}
	}

	if total == 0 && exact {
		warn!(
			"No packs were found in {}",
			options.input_directory.display()
		);
	}

	let env = Env {
		options,
		layout: &layout,
		extract_limits,
		search_depth: archive_search_depth,
		table
	};

	for item in found {
		match item {
			Found::Skipped(..) => {}
			Found::Directory(path) => {
				let name = file_name_of(&path);
				let dest = Dest {
					directory: layout
						.result_directory(path.parent().unwrap_or_else(|| Path::new(""))),
					name: layout.result_name(&name)
				};
				let label = state.label(&name, &path.display().to_string());

				process(
					&env,
					&mut state,
					squash,
					&path,
					None,
					&dest.file(),
					&label,
					None
				);
			}
			Found::Archive(path, scan) => {
				let dest = Dest {
					directory: layout
						.result_directory(path.parent().unwrap_or_else(|| Path::new(""))),
					name: layout.result_name(&archive_stem(&file_name_of(&path)))
				};

				process_archive(
					&env,
					&mut state,
					squash,
					&path,
					&scan,
					&dest,
					&path.display().to_string(),
					0
				);
			}
		}
	}

	let finished = if options.dry_run {
		"would be squashed"
	} else {
		"squashed"
	};
	let summary = &state.summary;
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
	let failed = summary.failed > 0;

	if failed {
		error!("{summary_text}");
		error!(
			"More help for this fork: <{}>",
			crate::error_hints::FORK_TROUBLESHOOTING_URL
		);
	} else {
		info!("{summary_text}");
	}

	let _ = writeln!(state.report, "{summary_text}");

	if let Some(report_file) = &options.report_file {
		match fs::write(report_file, &state.report) {
			Ok(()) => info!("Report written to {}", report_file.display()),
			Err(error) => error!(
				"Couldn't write the report to {}: {error}",
				report_file.display()
			)
		}
	}

	i32::from(failed)
}

/// Squashes one pack, or tells what would happen in a dry run. `replacement` is set for an archive that is replaced.
#[allow(clippy::too_many_arguments)]
fn process(
	env: &Env<'_>,
	state: &mut State,
	squash: &mut SquashFn<'_>,
	pack_directory: &Path,
	input_bytes: Option<u64>,
	output: &Path,
	label: &str,
	replacement: Option<&Replacement>
) {
	let final_output = replacement.map_or(output, |replacement| replacement.result.as_path());

	// An archive that is replaced writes to a temporary file first, and its result path is its original path
	let written_path = replacement.map_or(output, |replacement| replacement.temporary.as_path());

	if replacement.is_none() {
		if let Err(message) = state.guard.claim(output) {
			state.fail(label, &message);

			return;
		}
	} else if !state.guard.outputs.insert(final_output.to_path_buf()) {
		state.fail(
			label,
			&format!(
				"another pack already has the result {}",
				final_output.display()
			)
		);

		return;
	}

	if let Some(replacement) = replacement
		&& let Err(message) = replacement.check()
	{
		state.fail(label, &message);

		return;
	}

	if env.options.dry_run {
		state.summary.done += 1;

		let line = format!(
			"{label} -> {}{}",
			final_output.display(),
			match replacement {
				Some(replacement) => match &replacement.backup {
					Some(backup) => format!(" (the original would be kept as {})", backup.display()),
					None => " (the original would be deleted)".to_string()
				},
				None => String::new()
			}
		);
		state.emit(Kind::Done, line);

		return;
	}

	// The results of an archive with several packs go in folders that do not exist yet
	if replacement.is_none()
		&& let Some(parent) = output.parent()
		&& !parent.as_os_str().is_empty()
		&& let Err(error) = fs::create_dir_all(parent)
	{
		state.fail(
			label,
			&format!("couldn't create the folder {}: {error}", parent.display())
		);

		return;
	}

	let input_bytes = input_bytes.unwrap_or_else(|| directory_size(pack_directory));

	debug!(
		target: crate::debug_log::DEBUG_LOG_TARGET,
		"Bulk pack {label}: pack directory {} ({input_bytes} bytes), result file {}",
		pack_directory.display(),
		final_output.display()
	);

	let squash_options = match options_for(env.table, pack_directory, written_path) {
		Ok(squash_options) => squash_options,
		Err(message) => {
			state.fail(
				label,
				&format!("the options are not valid for this pack: {message}")
			);

			return;
		}
	};

	if let Err(message) = squash(squash_options) {
		if replacement.is_some() {
			let _ = fs::remove_file(written_path);
		}

		state.fail(label, &message);

		return;
	}

	if let Some(replacement) = replacement
		&& let Err(message) = replacement.apply()
	{
		let _ = fs::remove_file(&replacement.temporary);

		state.fail(label, &message);

		return;
	}

	let output_bytes = fs::metadata(final_output).map_or(0, |metadata| metadata.len());

	state.summary.done += 1;
	state.summary.input_bytes = state.summary.input_bytes.saturating_add(input_bytes);
	state.summary.output_bytes = state.summary.output_bytes.saturating_add(output_bytes);

	let line = format!(
		"{label} -> {} ({} -> {})",
		final_output.display(),
		mib(input_bytes),
		mib(output_bytes)
	);
	state.emit(Kind::Done, line);
}

/// Squashes the packs of one archive, and then the archives inside it. `source` tells where the archive is, for
/// the labels. `nesting` is how many archives this one is inside of.
///
/// The packs of the archive are extracted together, once, and deleted before the archives inside it are opened.
/// The archives inside it are extracted in groups, and each one is deleted after it is done.
#[allow(clippy::too_many_arguments)]
fn process_archive(
	env: &Env<'_>,
	state: &mut State,
	squash: &mut SquashFn<'_>,
	archive: &Path,
	scan: &ArchiveScan,
	dest: &Dest,
	source: &str,
	nesting: usize
) {
	let stem = archive_stem(&file_name_of(archive));
	let options = env.options;

	// In a dry run, nothing is extracted, so the packs are the ones that the scan found
	if options.dry_run {
		let roots = scan
			.pack_roots
			.clone()
			.unwrap_or_else(|| vec![String::new()]);
		let single = roots.len() == 1 && scan.nested_archives.is_empty();

		for root in &roots {
			let (name, pack_source) = pack_names(&stem, source, root);
			let label = state.label(&name, &pack_source);
			let (output, replacement) =
				result_for(env, archive, &stem, dest, root, single, nesting == 0);

			process(
				env,
				state,
				squash,
				archive,
				None,
				&output,
				&label,
				replacement.as_ref()
			);
		}

		for nested in &scan.nested_archives {
			state.emit(
				Kind::Note,
				format!(
					"{source} > {}: an archive inside the archive. A dry run does not open it, so it does not list its packs",
					nested.name
				)
			);
		}

		return;
	}

	let mut extracted = match extract_packs_and_nested(
		archive,
		scan,
		env.search_depth,
		&options.archive_extensions,
		env.extract_limits
	) {
		Ok(extracted) => extracted,
		Err(error) => {
			debug!(
				target: crate::debug_log::DEBUG_LOG_TARGET,
				"The packs could not be extracted from the archive {}. Error, with the errors that caused it: {}",
				archive.display(),
				crate::debug_log::error_chain(&error)
			);

			let label = state.label(&stem, source);
			state.fail(&label, &error.to_string());

			return;
		}
	};

	// A helper program extracts everything, so the archives inside the archive are already there
	let helper_nested = std::mem::take(&mut extracted.nested);
	let nested_count = if scan.pack_roots.is_some() {
		scan.nested_archives.len()
	} else {
		helper_nested.len()
	};

	// The scan of an archive that needs a helper program does not know the number of packs. If the real number
	// is different from the one that was counted, the numbers in the labels are not exact, but the work is right
	let single = extracted.packs.len() == 1 && nested_count == 0;

	for (root, pack_directory) in &extracted.packs {
		let (name, pack_source) = pack_names(&stem, source, root);
		let label = state.label(&name, &pack_source);
		let (output, replacement) = result_for(env, archive, &stem, dest, root, single, nesting == 0);

		process(
			env,
			state,
			squash,
			pack_directory,
			archive_pack_size(archive, root),
			&output,
			&label,
			replacement.as_ref()
		);
	}

	// Free the disk space of the packs before opening the archives inside. With a helper program, the archives
	// inside are in the same extracted files, which are kept until the end
	let _keep_alive = scan.pack_roots.is_none().then_some(extracted);

	for (name, path) in helper_nested {
		process_nested(env, state, squash, dest, &name, &path, source, nesting);

		let _ = fs::remove_file(&path);
	}

	if scan.pack_roots.is_some() {
		for names in nested_batches(&scan.nested_archives, scan.format) {
			match extract_nested_archives(archive, scan.format, &names, env.extract_limits) {
				Ok(files) => {
					for (name, path) in &files.files {
						process_nested(env, state, squash, dest, name, path, source, nesting);

						let _ = fs::remove_file(path);
					}
				}
				Err(error) => {
					debug!(
						target: crate::debug_log::DEBUG_LOG_TARGET,
						"The archives inside the archive {} could not be extracted. Error, with the errors that caused it: {}",
						archive.display(),
						crate::debug_log::error_chain(&error)
					);

					state.fail(
						source,
						&format!("couldn't extract the archives inside it: {error}")
					);
				}
			}
		}
	}
}

/// The name of a pack in an archive, and where it is, for its label. The name of a pack in the root of the archive
/// is the name of the archive.
fn pack_names(stem: &str, source: &str, root: &str) -> (String, String) {
	if root.is_empty() {
		(stem.to_string(), source.to_string())
	} else {
		(
			root.rsplit('/').next().unwrap_or(root).to_string(),
			format!("{source} > {root}")
		)
	}
}

/// The size that counts as the input of a pack of an archive, when it is not the size of the files of the pack.
/// A pack that is all of the archive counts as the size of the archive file, so that it is compared with its
/// result as a file, both compressed. The files of a pack that is in a directory of a bigger archive can not be
/// told apart from the others while they are compressed, so their size is counted extracted.
fn archive_pack_size(archive: &Path, root: &str) -> Option<u64> {
	if root.is_empty() {
		fs::metadata(archive).ok().map(|metadata| metadata.len())
	} else {
		None
	}
}

/// The result file of a pack of an archive, and how the archive is replaced, if it is.
fn result_for(
	env: &Env<'_>,
	archive: &Path,
	stem: &str,
	dest: &Dest,
	root: &str,
	single: bool,
	top_level: bool
) -> (PathBuf, Option<Replacement>) {
	if single {
		let replacement = (top_level && env.options.overwrite && env.layout.output_root.is_none())
			.then(|| replacement_for(env.options, archive, stem));

		(dest.file(), replacement)
	} else {
		(dest.pack_file(root), None)
	}
}

/// Opens an archive that was inside another archive, and squashes what is in it.
#[allow(clippy::too_many_arguments)]
fn process_nested(
	env: &Env<'_>,
	state: &mut State,
	squash: &mut SquashFn<'_>,
	parent: &Dest,
	name: &str,
	path: &Path,
	source: &str,
	nesting: usize
) {
	let nested_source = format!("{source} > {name}");

	let limit = nesting_limit(env.search_depth);

	if nesting + 1 > limit {
		state.emit(
			Kind::Skipped,
			format!(
				"skipped {nested_source}: it is inside more than {limit} archives, one inside the other. A higher bulk_scan_depth opens it (at most {MAX_NESTING})"
			)
		);

		return;
	}

	match scan_archive_with_nested(path, env.search_depth, &env.options.archive_extensions) {
		Ok(Some(scan)) if has_content(&scan) => process_archive(
			env,
			state,
			squash,
			path,
			&scan,
			&parent.nested(name),
			&nested_source,
			nesting + 1
		),
		Ok(Some(_)) => state.emit(
			Kind::Skipped,
			format!("skipped {nested_source}: it has no pack")
		),
		Ok(None) => state.emit(
			Kind::Skipped,
			format!("skipped {nested_source}: it is not an archive")
		),
		Err(error) => {
			debug!(
				target: crate::debug_log::DEBUG_LOG_TARGET,
				"The archive {nested_source} could not be read. Error, with the errors that caused it: {}",
				crate::debug_log::error_chain(&error)
			);

			state.emit(
				Kind::Skipped,
				format!("skipped {nested_source}: it can not be read: {error}")
			);
		}
	}
}

#[cfg(test)]
mod tests;
