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
	readers: Vec<thread::JoinHandle<()>>
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

fn start(number: usize, options: &str, parallel: bool, threads: usize) -> Result<Running, String> {
	let cap = options
		.parse::<Table>()
		.ok()
		.and_then(|table| table.get("threads")?.as_integer())
		.and_then(|count| usize::try_from(count).ok())
		.unwrap_or(usize::MAX);
	let threads = threads.min(cap);
	let path = env::temp_dir().join(format!(
		"packsquash-schedule-{}-{number}.toml",
		process::id()
	));

	fs::write(&path, options).map_err(|error| format!("Couldn't write the part: {error}"))?;

	let mut command = Command::new(env::current_exe().map_err(|error| error.to_string())?);

	command.arg(&path);

	if parallel {
		command
			.env("PACKSQUASH_SCHEDULE_THREADS", threads.to_string())
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped());
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

	Ok(Running {
		number,
		child,
		path,
		cap,
		threads,
		readers
	})
}

/// Runs the parts, each one as its own PackSquash run. With `parallel` of 2 or more, that many parts
/// run at the same time and share the threads of the computer. A part that fails does not stop the
/// schedule. The failed parts are told when they fail and again at the end.
pub fn run(schedule: &Schedule) -> i32 {
	for warning in &schedule.warnings {
		warn!("{warning}");
	}

	let total = schedule.parts.len();
	let slots = schedule.parallel.clamp(1, total);
	let parallel = slots > 1;
	let threads = thread::available_parallelism().map_or(1, NonZeroUsize::get);
	let mut next = 0;
	let mut running: Vec<Running> = Vec::new();
	let mut failed = Vec::new();

	while next < total || !running.is_empty() {
		while next < total && running.len() < slots {
			let used = running.iter().map(|part| part.threads).sum();
			let free = share(threads, used, slots - running.len(), total - next);

			next += 1;

			match start(next, &schedule.parts[next - 1], parallel, free) {
				Ok(part) => {
					if parallel {
						info!(
							"Starting part {next} of {total} with {} threads.",
							part.threads
						);
					} else {
						info!("Starting part {next} of {total}.");
					}

					running.push(part);
				}
				Err(message) => {
					error!("{message}");
					failed.push((next, -1));
				}
			}
		}

		thread::sleep(Duration::from_millis(200));

		let mut index = 0;
		let mut finished = false;

		while index < running.len() {
			let code = match running[index].child.try_wait() {
				Ok(None) => {
					index += 1;
					continue;
				}
				Ok(Some(status)) => status.code().unwrap_or(-1),
				Err(_) => -1
			};

			let part = running.remove(index);

			for reader in part.readers {
				let _ = reader.join();
			}

			let _ = fs::remove_file(&part.path);

			if code != 0 {
				error!("Part {} failed with the exit code {code}.", part.number);
				failed.push((part.number, code));
			}

			finished = true;
		}

		if parallel && finished && next == total && !running.is_empty() {
			let each = threads / running.len();

			for part in &mut running {
				let wanted = each.min(part.cap);

				if wanted > part.threads {
					part.threads = wanted;

					if let Some(stdin) = part.child.stdin.as_mut() {
						let _ = writeln!(stdin, "threads {wanted}");
					}
				}
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
