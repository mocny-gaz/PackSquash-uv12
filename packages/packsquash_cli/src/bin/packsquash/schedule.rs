use std::{
	env, fs,
	path::Path,
	process::{self, Command}
};

use log::{error, info, warn};
use toml::Table;

use crate::{fork_options::split_fork_options, parse_options};

const MAX_PARALLEL: usize = 64;

/// A schedule: options files that run one after another, each one being a part.
pub struct Schedule {
	pub parallel: usize,
	pub parts: Vec<String>
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

				parse_options(&padded).map(drop)
			});

		if let Err(message) = problem {
			errors.push(format!("Part {}: {}", index + 1, message.trim_end()));
		}
	}

	if errors.is_empty() {
		Ok(Schedule {
			parallel,
			parts: parts.into_iter().map(|(_, chunk)| chunk).collect()
		})
	} else {
		Err(errors)
	}
}

fn run_part(path: &Path, options: &str) -> Result<i32, String> {
	fs::write(path, options).map_err(|error| format!("Couldn't write the part: {error}"))?;

	let status = Command::new(env::current_exe().map_err(|error| error.to_string())?)
		.arg(path)
		.status()
		.map_err(|error| format!("Couldn't start PackSquash: {error}"))?;

	Ok(status.code().unwrap_or(-1))
}

/// Runs the parts one by one, each one as its own PackSquash run. A part that fails does not stop
/// the schedule. The failed parts are told when they fail and again at the end.
pub fn run(schedule: &Schedule) -> i32 {
	let total = schedule.parts.len();
	let mut failed = Vec::new();

	if schedule.parallel > 0 {
		warn!("The parallel option does nothing yet. The parts run one by one.");
	}

	for (index, options) in schedule.parts.iter().enumerate() {
		let number = index + 1;
		let path = env::temp_dir().join(format!(
			"packsquash-schedule-{}-{number}.toml",
			process::id()
		));

		info!("Starting part {number} of {total}.");

		let code = run_part(&path, options).unwrap_or_else(|message| {
			error!("{message}");
			-1
		});

		let _ = fs::remove_file(&path);

		if code != 0 {
			error!("Part {number} failed with the exit code {code}.");
			failed.push(format!("part {number} (exit code {code})"));
		}
	}

	if failed.is_empty() {
		info!("The schedule is done. Every part worked.");
		0
	} else {
		error!(
			"The schedule is done. These parts failed: {}.",
			failed.join(", ")
		);
		1
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	const TWO_PARTS: &str = "[schedule]\nparallel = 2\n\n[[part]]\npack_directory = 'a'\nzip_compression_iterations = 40\n\n['**/*?.png']\nimage_data_compression_iterations = 15\n\n[[part]]\npack_directory = 'b'\n";

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
			"[[part]]\npack_directory = 'a'\n[[part]]\npack_directory = 'b'\n[schedule]\nparallel = 3\n"
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
