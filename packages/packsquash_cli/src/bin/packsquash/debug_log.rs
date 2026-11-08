//! The debug log: a text file with everything that the program did and decided, with the details that are
//! needed to find out what went wrong without the pack. It is written when the options file has `log = true`.
//!
//! The file gets every message that the program and the library send to the [`log`] crate, at every level, and
//! the detailed messages of the library, which the console never shows. A logger that sits in front of the
//! console logger does it, so the console keeps showing what it showed before.

use std::{
	fmt::Write as _,
	fs::File,
	io::{self, BufWriter, Write},
	path::{Path, PathBuf},
	sync::{
		Mutex, MutexGuard,
		atomic::{AtomicBool, Ordering}
	},
	time::{SystemTime, UNIX_EPOCH}
};

use env_logger::Logger;
use log::{Level, Log, Metadata, Record};

pub use packsquash::DEBUG_LOG_TARGET;

/// Whether the log file is being written.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// The log file.
static SINK: Mutex<Option<BufWriter<File>>> = Mutex::new(None);
/// The path of the log file.
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
/// The lines that were logged before the log file was started, which is after the options file is read. They are
/// written first, so the file starts at the beginning.
static EARLY_LINES: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The most lines that are kept until the log file starts.
const EARLY_LINES_LIMIT: usize = 500;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	// A panic while the log is written must not stop the log from being written later
	mutex
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A logger that writes every message that matters to the log file, and gives the messages that the console
/// shows to the console logger.
pub struct TeeLogger {
	console: Logger
}

impl TeeLogger {
	pub const fn new(console: Logger) -> Self {
		Self { console }
	}
}

/// Tells whether the log file wants a message from the given target and level, if it is being written.
fn file_wants(target: &str, level: Level) -> bool {
	target == DEBUG_LOG_TARGET || target.starts_with("packsquash") || level <= Level::Warn
}

impl Log for TeeLogger {
	fn enabled(&self, metadata: &Metadata<'_>) -> bool {
		if metadata.target() == DEBUG_LOG_TARGET {
			ACTIVE.load(Ordering::Acquire)
		} else {
			self.console.enabled(metadata)
				|| (ACTIVE.load(Ordering::Acquire) && file_wants(metadata.target(), metadata.level()))
		}
	}

	fn log(&self, record: &Record<'_>) {
		if self.console.matches(record) {
			self.console.log(record);
		}

		let active = ACTIVE.load(Ordering::Acquire);

		// Before the file starts, only keep what the console shows, to not keep detailed messages that are
		// not wanted if no log file is asked for
		if !file_wants(record.target(), record.level())
			|| (!active && (record.target() == DEBUG_LOG_TARGET || !self.console.matches(record)))
		{
			return;
		}

		let line = format_line(
			SystemTime::now(),
			record.level(),
			record.target(),
			&record.args().to_string()
		);

		if active {
			write_line(&line, record.level() <= Level::Warn);
		} else {
			let mut early_lines = lock(&EARLY_LINES);

			if early_lines.len() < EARLY_LINES_LIMIT {
				early_lines.push(line);
			}
		}
	}

	fn flush(&self) {
		self.console.flush();
		flush();
	}
}

/// Formats a line of the log file. The lines after the first one of a message are indented.
fn format_line(time: SystemTime, level: Level, target: &str, message: &str) -> String {
	format!(
		"{} {:<5} [{}] {}",
		format_time(time),
		level,
		target,
		message.replace('\n', "\n    ")
	)
}

/// Formats a time as `2026-11-05T21:30:12.345Z`.
pub fn format_time(time: SystemTime) -> String {
	let since_epoch = time.duration_since(UNIX_EPOCH).unwrap_or_default();
	let seconds = since_epoch.as_secs();
	let (year, month, day) = civil_from_days(i64::try_from(seconds / 86_400).unwrap_or(0));
	let seconds_of_day = seconds % 86_400;

	format!(
		"{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
		seconds_of_day / 3600,
		seconds_of_day % 3600 / 60,
		seconds_of_day % 60,
		since_epoch.subsec_millis()
	)
}

/// Converts the days since 1970-01-01 to a date in the proleptic Gregorian calendar.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
	let z = days + 719_468;
	let era = z.div_euclid(146_097);
	let day_of_era = z.rem_euclid(146_097);
	let year_of_era =
		(day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
	let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
	let month_index = (5 * day_of_year + 2) / 153;
	let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
	let month = if month_index < 10 {
		month_index + 3
	} else {
		month_index - 9
	} as u32;
	let year = year_of_era + era * 400 + i64::from(month <= 2);

	(year, month, day)
}

fn write_line(line: &str, flush_now: bool) {
	if let Some(writer) = lock(&SINK).as_mut() {
		let _ = writeln!(writer, "{line}");

		// Warnings and errors are what is needed if the program stops suddenly
		if flush_now {
			let _ = writer.flush();
		}
	}
}

/// Writes the buffered data of the log file to the disk.
pub fn flush() {
	if let Some(writer) = lock(&SINK).as_mut() {
		let _ = writer.flush();
	}
}

/// Starts writing the log file, replacing the file if it exists. The messages that were logged before are
/// written first.
pub fn start(path: &Path) -> io::Result<()> {
	let mut writer = BufWriter::new(File::create(path)?);

	for line in lock(&EARLY_LINES).drain(..) {
		writeln!(writer, "{line}")?;
	}

	*lock(&SINK) = Some(writer);
	*lock(&PATH) = Some(path.to_path_buf());
	ACTIVE.store(true, Ordering::Release);

	Ok(())
}

/// The path of the log file, if it is being written.
pub fn path() -> Option<PathBuf> {
	lock(&PATH).clone()
}

/// Tells whether the log file is being written.
pub fn is_active() -> bool {
	ACTIVE.load(Ordering::Acquire)
}

/// Writes text to the log file as it is, in a block with a title, if it is being written. Used for the
/// contents of files that are shown whole.
pub fn write_block(title: &str, text: &str) {
	if is_active() {
		let mut block = format!("---- {title} ----\n");
		block.push_str(text);

		if !text.ends_with('\n') {
			block.push('\n');
		}

		let _ = write!(block, "---- end of {title} ----");
		write_line(&block, false);
	}
}

/// Describes an error and the errors that caused it, with the details of each, for the log file.
pub fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
	packsquash::describe_error_chain(error)
}

/// Installs a panic hook that writes the panic, with its backtrace, to the log file before the usual hook runs.
pub fn log_panics() {
	let previous_hook = std::panic::take_hook();

	std::panic::set_hook(Box::new(move |info| {
		if is_active() {
			write_line(
				&format_line(
					SystemTime::now(),
					Level::Error,
					"panic",
					&format!(
						"The program panicked: {info}\nBacktrace:\n{}",
						std::backtrace::Backtrace::force_capture()
					)
				),
				true
			);
		}

		previous_hook(info);
	}));
}

#[cfg(test)]
mod tests {
	use std::time::Duration;

	use super::*;

	#[test]
	fn times_are_written_in_utc() {
		assert_eq!(format_time(UNIX_EPOCH), "1970-01-01T00:00:00.000Z");
		assert_eq!(
			format_time(UNIX_EPOCH + Duration::from_millis(1_700_000_000_123)),
			"2023-11-14T22:13:20.123Z"
		);
		// A leap day, and the end of a year
		assert_eq!(
			format_time(UNIX_EPOCH + Duration::from_secs(1_709_164_800)),
			"2024-02-29T00:00:00.000Z"
		);
		assert_eq!(
			format_time(UNIX_EPOCH + Duration::from_secs(1_767_225_599)),
			"2025-12-31T23:59:59.000Z"
		);
	}

	#[test]
	fn lines_have_the_time_level_target_and_indented_continuations() {
		let line = format_line(UNIX_EPOCH, Level::Debug, "debuglog", "first\nsecond");

		assert_eq!(
			line,
			"1970-01-01T00:00:00.000Z DEBUG [debuglog] first\n    second"
		);
	}

	#[test]
	fn the_file_wants_the_messages_of_the_program_and_the_problems_of_the_libraries() {
		assert!(file_wants(DEBUG_LOG_TARGET, Level::Debug));
		assert!(file_wants("packsquash::bulk", Level::Trace));
		assert!(file_wants("tokio", Level::Warn));
		assert!(!file_wants("tokio", Level::Debug));
	}
}
