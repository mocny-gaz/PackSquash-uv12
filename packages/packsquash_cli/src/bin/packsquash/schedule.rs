use std::{
	env, fs,
	io::{self, BufRead, BufReader, Read, Write},
	num::NonZeroUsize,
	path::{self, PathBuf},
	process::{self, Child, Command, Stdio},
	sync::atomic::{AtomicUsize, Ordering},
	thread,
	time::Duration
};

use log::{error, info, warn};
use packsquash::config::SquashOptions;
use toml::Table;

use crate::{fork_options::split_fork_options, parse_options};

const MAX_PARALLEL: usize = 64;

/// A schedule: options files that run one after another, each one being a part.
pub struct Schedule {
	pub parallel: usize,
	pub parts: Vec<String>,
	pub warnings: Vec<String>
}

static THREADS: AtomicUsize = AtomicUsize::new(0);
static SHOWN: AtomicUsize = AtomicUsize::new(0);

fn text(table: &Table, key: &str) -> Option<String> {
	table.get(key)?.as_str().map(str::to_string)
}

fn place(path: &str) -> String {
	let path = path::absolute(path).map_or_else(
		|_| path.to_string(),
		|path| path.to_string_lossy().into_owned()
	);

	if cfg!(windows) {
		path.to_lowercase()
	} else {
		path
	}
}

fn places(table: &Table) -> (String, String) {
	if let Some(input) = text(table, "bulk_input_directory") {
		let output = text(table, "bulk_output_directory").unwrap_or_else(|| input.clone());

		let output = format!(
			"{}|{}|{}",
			place(&output),
			text(table, "bulk_output_prefix").unwrap_or_default(),
			text(table, "bulk_output_suffix").unwrap_or_default()
		);

		(output, place(&input))
	} else {
		(
			place(&text(table, "output_file_path").unwrap_or_else(|| "pack.zip".to_string())),
			place(&text(table, "pack_directory").unwrap_or_default())
		)
	}
}

fn compact(line: &str) -> String {
	line.split('#')
		.next()
		.unwrap_or_default()
		.split_whitespace()
		.collect()
}

/// Writes a schedule file: the options of each part under a `[[part]]` line.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn export(parts: &[String], parallel: usize) -> String {
	let mut text = format!("[schedule]\nparallel = {parallel}\n");

	for part in parts {
		text.push_str("\n[[part]]\n");
		text.push_str(part.trim());
		text.push('\n');
	}

	text
}

pub fn is_schedule(text: &str) -> bool {
	text.lines().any(|line| compact(line) == "[schedule]")
}

/// Reads a schedule file. Every problem found is returned, not only the first one.
pub fn parse(text: &str) -> Result<Schedule, Vec<String>> {
	let mut errors = Vec::new();
	let mut schedule_line = None;
	let mut schedule_text = String::new();
	let mut in_schedule = false;
	let mut head = String::new();
	let mut parts: Vec<(usize, String)> = Vec::new();

	for (index, line) in text.lines().enumerate() {
		let line_number = index + 1;
		let name = compact(line);
		let mut kept = line;

		if name == "[schedule]" {
			if let Some(first) = schedule_line {
				errors.push(format!(
					"Line {line_number}: The schedule table is already on line {first}."
				));
			} else {
				schedule_line = Some(line_number);
			}

			in_schedule = true;
			kept = "";
		} else if name.starts_with("[[part") && name.ends_with("]]") {
			if name != "[[part]]" {
				errors.push(format!(
					"Line {line_number}: Parts have no numbers or names. Write [[part]]."
				));
			}

			in_schedule = false;
			parts.push((line_number, String::new()));
			continue;
		} else if in_schedule {
			if line.trim_start().starts_with('[') {
				in_schedule = false;
			} else {
				schedule_text.push_str(line);
				schedule_text.push('\n');
				kept = "";
			}
		}

		let chunk = parts.last_mut().map_or(&mut head, |(_, chunk)| chunk);
		chunk.push_str(kept);
		chunk.push('\n');
	}

	let mut parallel = 0;

	if let Some(line) = schedule_line {
		match format!("{}{schedule_text}", "\n".repeat(line)).parse::<Table>() {
			Ok(table) => {
				for (key, value) in table {
					match (
						key.as_str(),
						value
							.as_integer()
							.and_then(|count| usize::try_from(count).ok())
					) {
						("parallel", Some(count)) if count <= MAX_PARALLEL => parallel = count,
						("parallel", _) => errors.push(format!(
							"The parallel option of the schedule must be a whole number from 0 to {MAX_PARALLEL}."
						)),
						_ => errors.push(format!(
							"The schedule has an unknown option, {key}. Only parallel is known."
						))
					}
				}
			}
			Err(error) => errors.push(format!("The schedule table is wrong: {error}"))
		}
	}

	match head.parse::<Table>() {
		Ok(table) if !table.is_empty() => errors.push(
			"Options are set before the first [[part]] line. Put them under a [[part]] line."
				.to_string()
		),
		Ok(_) => {}
		Err(error) => errors.push(format!("The text before the first part is wrong: {error}"))
	}

	if parts.is_empty() {
		errors.push(
			"The schedule has no parts. Add a [[part]] line before the options of each part."
				.to_string()
		);
	}

	let mut found = Vec::new();

	for (index, (line, chunk)) in parts.iter().enumerate() {
		let padded = format!("{}{chunk}", "\n".repeat(*line));

		let problem = padded
			.parse::<Table>()
			.map_err(|error| error.to_string())
			.and_then(|table| {
				if split_fork_options(&mut table.clone())?.bulk.is_none()
					&& !table.contains_key("pack_directory")
				{
					return Err("The pack_directory option is missing.".to_string());
				}

				parse_options(&padded)?;

				Ok(places(&table))
			});

		match problem {
			Ok(place) => found.push((index + 1, place)),
			Err(message) => errors.push(format!("Part {}: {}", index + 1, message.trim_end()))
		}
	}

	let mut warnings = Vec::new();

	if parallel > 1 {
		for (later, (second, (output, input))) in found.iter().enumerate() {
			for (first, (other_output, other_input)) in &found[..later] {
				if output == other_output {
					errors.push(format!(
						"Parts {first} and {second} write the same output. Parts that run at the same time can not share an output."
					));
				}

				if input == other_input {
					warnings.push(format!("Parts {first} and {second} read the same input."));
				}
			}
		}
	}

	if errors.is_empty() {
		Ok(Schedule {
			parallel,
			parts: parts.into_iter().map(|(_, chunk)| chunk).collect(),
			warnings
		})
	} else {
		Err(errors)
	}
}

pub fn listen() {
	THREADS.store(
		env::var("PACKSQUASH_SCHEDULE_THREADS")
			.ok()
			.and_then(|count| count.parse().ok())
			.unwrap_or(0),
		Ordering::Relaxed
	);

	thread::spawn(|| {
		for line in io::stdin().lock().lines().map_while(Result::ok) {
			if let Some(count) = line
				.strip_prefix("threads ")
				.and_then(|count| count.trim().parse().ok())
			{
				THREADS.store(count, Ordering::Relaxed);
			}
		}
	});
}

pub fn apply_threads(options: &mut SquashOptions) {
	if let Some(count) = NonZeroUsize::new(THREADS.load(Ordering::Relaxed)) {
		options.global_options.threads = count;

		if SHOWN.swap(count.get(), Ordering::Relaxed) != count.get() {
			info!("The schedule gives this part {count} threads.");
		}
	}
}

fn share(threads: usize, used: usize, free_slots: usize, queued: usize) -> usize {
	(threads.saturating_sub(used).max(1) / free_slots.min(queued)).max(1)
}

struct Running {
	number: usize,
	child: Child,
	path: PathBuf,
	cap: usize,
	threads: usize,
	stopped: bool,
	readers: Vec<thread::JoinHandle<()>>
}

pub struct Finished {
	pub number: usize,
	pub code: i32,
	#[cfg_attr(not(windows), allow(dead_code))]
	pub stopped: bool
}

fn pump(pipe: impl Read + Send + 'static, number: usize, to_error: bool) -> thread::JoinHandle<()> {
	thread::spawn(move || {
		for line in BufReader::new(pipe).lines().map_while(Result::ok) {
			let _ = if to_error {
				writeln!(io::stderr(), "[part {number}] {line}")
			} else {
				writeln!(io::stdout(), "[part {number}] {line}")
			};
		}
	})
}

/// The parts that run now, each one as its own PackSquash run. The terminal and the window decide
/// which part starts when. The runner gives the threads of the computer to the parts that run
/// together, and reads their output.
pub struct Runner {
	slots: usize,
	threads: usize,
	window_state: Option<String>,
	running: Vec<Running>
}

impl Runner {
	/// Runs `parallel` parts at the same time, or one if it is 0. The window gives what it knows
	/// about itself for the debug log of the runs.
	pub fn new(parallel: usize, window_state: Option<String>) -> Self {
		Self {
			slots: parallel.max(1),
			threads: thread::available_parallelism().map_or(1, NonZeroUsize::get),
			window_state,
			running: Vec::new()
		}
	}

	pub fn has_slot(&self) -> bool {
		self.running.len() < self.slots
	}

	pub fn is_idle(&self) -> bool {
		self.running.is_empty()
	}

	/// Starts a part, with `queued` parts left to start, this one included. Returns the threads
	/// of the part.
	pub fn start(&mut self, number: usize, options: &str, queued: usize) -> Result<usize, String> {
		let used = self.running.iter().map(|part| part.threads).sum();
		let cap = options
			.parse::<Table>()
			.ok()
			.and_then(|table| table.get("threads")?.as_integer())
			.and_then(|count| usize::try_from(count).ok())
			.unwrap_or(usize::MAX);
		let threads = share(self.threads, used, self.slots - self.running.len(), queued).min(cap);
		let path = env::temp_dir().join(format!(
			"packsquash-schedule-{}-{number}.toml",
			process::id()
		));

		fs::write(&path, options).map_err(|error| format!("Couldn't write the part: {error}"))?;

		let mut command = Command::new(env::current_exe().map_err(|error| error.to_string())?);

		command.arg(&path);

		let piped = self.slots > 1 || self.window_state.is_some();

		if piped {
			command
				.env("PACKSQUASH_SCHEDULE_THREADS", threads.to_string())
				.stdin(Stdio::piped())
				.stdout(Stdio::piped())
				.stderr(Stdio::piped());
		}

		if let Some(state) = &self.window_state {
			command
				.args(["--color", "--no-emoji"])
				.env("PACKSQUASH_STARTED_BY", "native")
				.env("PACKSQUASH_WINDOW_STATE", state);
		}

		let mut child = command
			.spawn()
			.map_err(|error| format!("Couldn't start PackSquash: {error}"))?;

		let readers = [
			child.stdout.take().map(|pipe| pump(pipe, number, false)),
			child.stderr.take().map(|pipe| pump(pipe, number, true))
		]
		.into_iter()
		.flatten()
		.collect();

		self.running.push(Running {
			number,
			child,
			path,
			cap,
			threads,
			stopped: false,
			readers
		});

		Ok(threads)
	}

	/// Collects the parts that ended. With no part left to start, the parts that go on take the free
	/// threads.
	pub fn poll(&mut self, queued: usize) -> Vec<Finished> {
		let mut finished = Vec::new();
		let mut index = 0;

		while index < self.running.len() {
			let code = match self.running[index].child.try_wait() {
				Ok(None) => {
					index += 1;
					continue;
				}
				Ok(Some(status)) => status.code().unwrap_or(-1),
				Err(_) => -1
			};

			let part = self.running.remove(index);

			if !part.stopped {
				for reader in part.readers {
					let _ = reader.join();
				}
			}

			let _ = fs::remove_file(&part.path);

			finished.push(Finished {
				number: part.number,
				code,
				stopped: part.stopped
			});
		}

		if self.slots > 1 && queued == 0 && !finished.is_empty() && !self.running.is_empty() {
			let each = self.threads / self.running.len();

			for part in &mut self.running {
				let wanted = each.min(part.cap);

				if wanted > part.threads {
					part.threads = wanted;

					if let Some(stdin) = part.child.stdin.as_mut() {
						let _ = writeln!(stdin, "threads {wanted}");
					}
				}
			}
		}

		finished
	}

	#[cfg_attr(not(windows), allow(dead_code))]
	pub fn stop(&mut self, number: usize) {
		for part in self.running.iter_mut().filter(|part| part.number == number) {
			part.stopped = true;
			let _ = part.child.kill();
		}
	}

	#[cfg_attr(not(windows), allow(dead_code))]
	pub fn stop_all(&mut self) {
		for part in &mut self.running {
			part.stopped = true;
			let _ = part.child.kill();
		}
	}
}

/// Runs the schedule in the terminal. A part that fails does not stop the schedule. The failed parts
/// are told when they fail and again at the end.
pub fn run(schedule: &Schedule) -> i32 {
	for warning in &schedule.warnings {
		warn!("{warning}");
	}

	let total = schedule.parts.len();
	let parallel = schedule.parallel.clamp(1, total);
	let mut runner = Runner::new(parallel, None);
	let mut next = 0;
	let mut failed = Vec::new();

	while next < total || !runner.is_idle() {
		while next < total && runner.has_slot() {
			next += 1;

			match runner.start(next, &schedule.parts[next - 1], total - next + 1) {
				Ok(threads) if parallel > 1 => {
					info!("Starting part {next} of {total} with {threads} threads.");
				}
				Ok(_) => info!("Starting part {next} of {total}."),
				Err(message) => {
					error!("{message}");
					failed.push((next, -1));
				}
			}
		}

		thread::sleep(Duration::from_millis(200));

		for part in runner.poll(total - next) {
			if part.code != 0 {
				error!(
					"Part {} failed with the exit code {}.",
					part.number, part.code
				);
				failed.push((part.number, part.code));
			}
		}
	}

	if failed.is_empty() {
		info!("The schedule is done. Every part worked.");
		0
	} else {
		failed.sort_unstable();

		let list: Vec<_> = failed
			.iter()
			.map(|(number, code)| format!("part {number} (exit code {code})"))
			.collect();

		error!(
			"The schedule is done. These parts failed: {}.",
			list.join(", ")
		);
		1
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const TWO_PARTS: &str = "[schedule]\nparallel = 2\n\n[[part]]\npack_directory = 'a'\noutput_file_path = 'a.zip'\nzip_compression_iterations = 40\n\n['**/*?.png']\nimage_data_compression_iterations = 15\n\n[[part]]\npack_directory = 'b'\noutput_file_path = 'b.zip'\n";

	fn errors(text: &str) -> Vec<String> {
		parse(text).err().expect("The schedule should not be read")
	}

	#[test]
	fn a_file_without_the_schedule_table_is_not_a_schedule() {
		assert!(!is_schedule("pack_directory = 'a'\n"));
		assert!(is_schedule(TWO_PARTS));
		assert!(is_schedule("  [ schedule ] # comment\n"));
	}

	#[test]
	fn a_schedule_is_split_into_its_parts() {
		let schedule = parse(TWO_PARTS).expect("The schedule should be read");

		assert_eq!(schedule.parallel, 2);
		assert_eq!(schedule.parts.len(), 2);
		assert!(schedule.parts[0].contains("zip_compression_iterations = 40"));
		assert!(schedule.parts[0].contains("['**/*?.png']"));
		assert!(schedule.parts[1].contains("pack_directory = 'b'"));
		assert!(!schedule.parts.iter().any(|part| part.contains("[[part]]")));
		assert!(!schedule.parts.iter().any(|part| part.contains("parallel")));
	}

	#[test]
	fn the_schedule_table_can_be_anywhere() {
		let schedule = parse(
			"[[part]]\npack_directory = 'a'\noutput_file_path = 'a.zip'\n[[part]]\npack_directory = 'b'\noutput_file_path = 'b.zip'\n[schedule]\nparallel = 3\n"
		)
		.expect("The schedule should be read");

		assert_eq!(schedule.parallel, 3);
		assert_eq!(schedule.parts.len(), 2);

		let schedule =
			parse("[[part]]\npack_directory = 'a'\n[schedule]\n[[part]]\npack_directory = 'b'\n")
				.expect("The schedule should be read");

		assert_eq!(schedule.parallel, 0);
		assert_eq!(schedule.parts.len(), 2);
	}

	#[test]
	fn an_exported_schedule_is_read_back() {
		let parts = vec![
			"pack_directory = 'a'\noutput_file_path = 'a.zip'\n\n['**/*?.png']\nimage_data_compression_iterations = 15\n\n".to_string(),
			"# comment\npack_directory = 'b'\noutput_file_path = 'b.zip'".to_string()
		];
		let schedule = parse(&export(&parts, 2)).expect("The schedule should be read");

		assert_eq!(schedule.parallel, 2);
		assert_eq!(schedule.parts.len(), 2);
		assert!(schedule.parts[0].contains("['**/*?.png']"));
		assert_eq!(schedule.parts[1].trim(), parts[1]);
	}

	#[test]
	fn windows_line_endings_work() {
		assert!(parse(&TWO_PARTS.replace('\n', "\r\n")).is_ok());
	}

	#[test]
	fn a_schedule_without_parts_is_an_error() {
		assert_eq!(
			errors("[schedule]\nparallel = 1\n"),
			["The schedule has no parts. Add a [[part]] line before the options of each part."]
		);
	}

	#[test]
	fn parts_with_numbers_or_names_are_an_error() {
		let found = errors("[schedule]\n[[part2]]\npack_directory = 'a'\n");

		assert_eq!(found.len(), 1);
		assert!(found[0].starts_with("Line 2: Parts have no numbers or names."));
		assert!(errors("[schedule]\n[[ part_a ]]\npack_directory = 'a'\n")[0].starts_with("Line 2:"));
	}

	#[test]
	fn options_before_the_first_part_are_an_error() {
		let found = errors("pack_directory = 'x'\n[schedule]\n[[part]]\npack_directory = 'a'\n");
		assert_eq!(found.len(), 1);
		assert!(found[0].starts_with("Options are set before the first [[part]] line."));
	}

	#[test]
	fn a_part_without_a_pack_directory_is_an_error_that_names_the_part() {
		assert_eq!(
			errors(
				"[schedule]\n[[part]]\npack_directory = 'a'\n[[part]]\nzip_compression_iterations = 3\n"
			),
			["Part 2: The pack_directory option is missing."]
		);
	}

	#[test]
	fn a_bulk_part_needs_no_pack_directory() {
		assert!(parse("[schedule]\n[[part]]\nbulk_input_directory = 'packs'\n").is_ok());
	}

	#[test]
	fn bad_schedule_values_are_errors() {
		let part = "[[part]]\npack_directory = 'a'\n";

		for bad in [
			"parallel = 'two'",
			"parallel = -1",
			"parallel = 65",
			"parallel = 1.5"
		] {
			let found = errors(&format!("[schedule]\n{bad}\n{part}"));

			assert_eq!(found.len(), 1, "{bad}");
			assert!(
				found[0].starts_with("The parallel option of the schedule must be"),
				"{bad}"
			);
		}

		assert!(
			errors(&format!("[schedule]\nthreads = 2\n{part}"))[0]
				.contains("unknown option, threads")
		);
		assert!(
			errors(&format!("[schedule]\nparallel =\n{part}"))[0]
				.starts_with("The schedule table is wrong")
		);
		assert!(errors(&format!("[schedule]\n[schedule]\n{part}"))[0].starts_with("Line 2:"));
	}

	#[test]
	fn all_the_errors_are_found_at_once() {
		let found = errors(
			"[schedule]\nparallel = 'x'\n[[part1]]\npack_directory = 'a'\n[[part]]\nzip_compression_iterations = 3\n[[part]]\npack_directory = 'c'\nzip_compression_iterations = 'many'\n"
		);

		assert_eq!(found.len(), 4);
		assert!(found[0].starts_with("Line 3:"));
		assert!(found[1].starts_with("The parallel option"));
		assert!(found[2].starts_with("Part 2: The pack_directory option is missing."));
		assert!(found[3].starts_with("Part 3:"));
	}

	#[test]
	fn the_same_output_is_an_error_only_when_parts_run_at_the_same_time() {
		let part = |extra: &str| format!("[[part]]\npack_directory = 'a'\n{extra}\n");
		let same = format!(
			"{}{}",
			part("output_file_path = 'x.zip'"),
			part("output_file_path = 'x.zip'")
		);

		assert_eq!(
			errors(&format!("[schedule]\nparallel = 2\n{same}")),
			[
				"Parts 1 and 2 write the same output. Parts that run at the same time can not share an output."
			]
		);
		assert_eq!(
			errors(&format!(
				"[schedule]\nparallel = 2\n{}{}",
				part(""),
				part("")
			))
			.len(),
			1
		);
		assert!(parse(&format!("[schedule]\nparallel = 1\n{same}")).is_ok());
		assert!(parse(&format!("[schedule]\n{same}")).is_ok());
		assert!(
			errors(&format!(
				"[schedule]\nparallel = 2\n{}{}",
				"[[part]]\nbulk_input_directory = 'a'\nbulk_output_directory = 'o'\n",
				"[[part]]\nbulk_input_directory = 'b'\nbulk_output_directory = 'o'\n"
			))[0]
				.starts_with("Parts 1 and 2 write the same output.")
		);
		assert!(
			parse(&format!(
				"[schedule]\nparallel = 2\n{}{}",
				"[[part]]\nbulk_input_directory = 'a'\nbulk_output_directory = 'o'\nbulk_output_suffix = '_a'\n",
				"[[part]]\nbulk_input_directory = 'b'\nbulk_output_directory = 'o'\nbulk_output_suffix = '_b'\n"
			))
			.is_ok()
		);
	}

	#[test]
	fn the_same_input_is_only_a_warning() {
		let part =
			|output: &str| format!("[[part]]\npack_directory = 'a'\noutput_file_path = '{output}'\n");

		let schedule = parse(&format!(
			"[schedule]\nparallel = 2\n{}{}",
			part("x.zip"),
			part("y.zip")
		))
		.expect("The schedule should be read");

		assert_eq!(schedule.warnings, ["Parts 1 and 2 read the same input."]);

		let schedule = parse(&format!(
			"[schedule]\nparallel = 0\n{}{}",
			part("x.zip"),
			part("y.zip")
		))
		.expect("The schedule should be read");

		assert!(schedule.warnings.is_empty());
	}

	#[test]
	fn the_threads_are_shared_between_the_parts_that_run_together() {
		assert_eq!(share(16, 0, 2, 4), 8);
		assert_eq!(share(16, 8, 1, 3), 8);
		assert_eq!(share(16, 8, 1, 1), 8);
		assert_eq!(share(16, 0, 1, 1), 16);
		assert_eq!(share(16, 0, 3, 2), 8);
		assert_eq!(share(16, 0, 3, 5), 5);
		assert_eq!(share(4, 4, 1, 1), 1);
		assert_eq!(share(1, 0, 2, 2), 1);
	}

	#[test]
	fn the_lines_of_errors_are_the_lines_of_the_file() {
		let found = errors(
			"[schedule]\nparallel = 0\n\n[[part]]\npack_directory = 'a'\n\n[[part]]\npack_directory =\n"
		);

		assert_eq!(found.len(), 1);
		assert!(
			found[0].starts_with("Part 2: TOML parse error at line 8,"),
			"{}",
			found[0]
		);
	}
}
