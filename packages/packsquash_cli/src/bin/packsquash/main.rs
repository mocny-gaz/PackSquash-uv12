//! The `PackSquash` proper CLI application code.

use anstyle::{AnsiColor, Color, Effects};
use std::{
	borrow::Cow,
	env, fs,
	io::{self, IsTerminal, Read, Stderr},
	process,
	time::{Duration, Instant}
};

use env_logger::{Builder, Target, WriteStyle};
use error_hints::{FORK_TROUBLESHOOTING_URL, UPSTREAM_TROUBLESHOOTING_URL};
use fork_options::{ForkOptions, has_fork_options, split_fork_options};
use getopts::{Options, ParsingStyle};
use log::{Level, LevelFilter, debug, error, info, trace, warn};
use packsquash::{
	PackSquasher, PackSquasherError, PackSquasherStatus, PackSquasherWarning,
	config::SquashOptions,
	pack_archive::{ExtractLimits, ExtractedPacks, extract_packs, scan_archive},
	vfs::os_fs::OsFilesystem
};
use terminal_style::{environment_allows_color, environment_allows_emoji};
use terminal_title_controller::TerminalTitleController;
use tokio::{runtime, select, sync::mpsc::channel, time::sleep};
use toml::{Table, Value};
use tz::UtcDateTime;

mod bulk;
mod debug_log;
mod error_hints;
mod fork_options;
mod terminal_style;
mod terminal_title_controller;
mod terminal_title_setter;

/// The log target where status messages will be sent to.
const LOG_TARGET: Target = Target::Stderr;
/// A producer of the [`IsTerminal`] implementation that matches the [`LOG_TARGET`] constant.
const LOG_TARGET_STREAM: fn() -> Stderr = io::stderr;

/// Zopfli compression is pretty intensive on the memory allocator, and musl
/// is known to use a simple but prone to extreme thread contention allocator.
/// Replace its allocator with one whose performance is close to glibc so that
/// the PackSquash CLI musl binaries have a similar performance to glibc
/// binaries; otherwise, slowdowns of 5x or more are to be expected.
#[cfg(all(target_os = "linux", target_env = "musl"))]
#[global_allocator]
static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() {
	process::exit(run(TerminalTitleController::new()));
}

/// Runs `PackSquash`, parsing the command line parameters and deciding what options file
/// to read to process a pack.
fn run(title_controller: Option<TerminalTitleController>) -> i32 {
	// Show initial title
	if let Some(title_controller) = &title_controller {
		title_controller.show();
	}

	let log_target_is_tty = LOG_TARGET_STREAM().is_terminal();
	let enable_emoji_default = environment_allows_emoji(log_target_is_tty);
	let enable_color_default = environment_allows_color(log_target_is_tty);

	let mut options = Options::new();

	options.optflag("h", "help", "Prints information about the command line arguments accepted by this application and exits")
		.optflag("v", "version", "Prints version and copyright information of the application, then exits")
		.optflag(
			"",
			"emoji",
			"Always enable emojis in status messages. \
			This is equivalent to setting the PACKSQUASH_EMOJI or EMOJI environment variables to \"show\""
		)
		.optflag(
			"",
			"no-emoji",
			"Always disable emojis status messages. \
			 This is equivalent to defining the NO_EMOJI environment variable, or setting PACKSQUASH_EMOJI or EMOJI to something else than \"show\""
		)
		.optflag(
			"",
			"color",
			"Always enable color in messages. \
			This is equivalent to setting the PACKSQUASH_COLOR or COLOR environment variables to \"show\""
		)
		.optflag(
			"",
			"no-color",
			"Always disable color in messages. \
			This is equivalent to defining the NO_COLOR environment variable, or setting PACKSQUASH_COLOR or COLOR to something else than \"show\""
		)
		.parsing_style(ParsingStyle::StopAtFirstFree);

	match options.parse(env::args().skip(1)) {
		Ok(option_matches) => {
			if option_matches.opt_present("h") {
				print_version_information(true);
				println!();
				println!("Usage:");
				print!(
					"    {} [OPTION]... [options file path]",
					env!("CARGO_BIN_NAME")
				);
				println!("{}", options.usage(""));

				0
			} else if option_matches.opt_present("v") {
				print_version_information(true);

				0
			} else {
				let enable_emoji = if enable_emoji_default {
					!option_matches.opt_present("no-emoji")
				} else {
					option_matches.opt_present("emoji")
				};

				let enable_color = if enable_color_default {
					!option_matches.opt_present("no-color")
				} else {
					option_matches.opt_present("color")
				};

				init_logger(enable_emoji, enable_color);

				print_version_information(false);
				println!();

				let started_at = std::time::Instant::now();
				let exit_code = read_options_file_and_squash(
					option_matches.free.first().filter(|path| {
						// Let "-" behave as if no path was provided
						path != &"-"
					}),
					title_controller
				);

				if let Some(path) = debug_log::path() {
					log::debug!(
						target: debug_log::DEBUG_LOG_TARGET,
						"Finished with the exit code {exit_code} after {:.3} seconds",
						started_at.elapsed().as_secs_f64()
					);
					debug_log::flush();

					info!(
						"The debug log was written to {}",
						absolute_path(&path).display()
					);
				}

				exit_code
			}
		}
		Err(parse_err) => {
			init_logger(enable_emoji_default, enable_color_default);

			error!(
				"{}\nRun {} -h to see command line argument help",
				parse_err,
				env!("CARGO_BIN_NAME")
			);

			1
		}
	}
}

/// Reads an options file and launches a squash operation to optimize it with the
/// read options.
fn read_options_file_and_squash(
	options_file_path: Option<&String>,
	title_controller: Option<TerminalTitleController>
) -> i32 {
	let user_friendly_options_path =
		options_file_path.map_or("standard input (keyboard input or pipe)", |path| path);

	// Tell the user where are we reading the configuration from
	info!(
		"Reading options from {}...{}",
		user_friendly_options_path,
		if options_file_path.is_none() {
			// Newbies are often confused by terms such as "standard input", so try
			// to point them in the direction of what they probably want to do
			"\nIf you are not sure what this means, try using an external options file.\
			 \nPlease check out <https://packsquash.aylas.org/links/Options-files> for examples and more information."
		} else {
			""
		}
	);

	// Read the TOML configuration data from the specified source
	let options_string = match if let Some(path) = options_file_path {
		fs::read_to_string(path)
	} else {
		let mut buf = String::new();
		match io::stdin().read_to_string(&mut buf) {
			Ok(_) => Ok(buf),
			Err(err) => Err(err)
		}
	} {
		Ok(options_string) => options_string,
		Err(err) => {
			error!("Couldn't read the options file from {user_friendly_options_path}: {err}");

			return 2;
		}
	};

	// Deserialize the options struct contained in the string
	let (mut squash_options, fork_options, options_table) = match parse_options(&options_string) {
		Ok(parsed_options) => parsed_options,
		Err(deserialize_error) => {
			error!(
				"An error occurred while parsing the options file from {user_friendly_options_path}: {deserialize_error}"
			);

			return 3;
		}
	};

	if let Some(log_options) = &fork_options.log {
		start_debug_log(
			log_options,
			options_file_path,
			&options_string,
			&fork_options
		);
	}

	if let Some(bulk_options) = &fork_options.bulk {
		if options_table.contains_key("pack_directory")
			|| options_table.contains_key("output_file_path")
		{
			warn!("The bulk mode is on, so pack_directory and output_file_path are ignored");
		}

		info!("Options read. Bulk mode is on.");

		let extract_limits = ExtractLimits {
			max_bytes: fork_options.archive_max_extracted_bytes,
			..ExtractLimits::default()
		};

		// A title that goes through phases for a single pack makes no sense for many packs
		drop(title_controller);

		// In the bulk mode, the packs are searched as deep inside the archives as in the directories
		return bulk::run(
			bulk_options,
			bulk_options
				.scan_depth
				.max(fork_options.archive_pack_search_depth),
			extract_limits,
			&options_table,
			&mut |pack_options| {
				squash(pack_options, None)
					.map(|_| ())
					.map_err(|err| describe_error(&err, false))
			}
		);
	}

	info!("Options read. Processing pack...");

	// If the pack is in an archive, extract it. The extracted files are deleted when the
	// binding is dropped at the end of this function, after the squash operation ends
	let _extracted_pack = match prepare_archive_input(&mut squash_options, &fork_options) {
		Ok(extracted_pack) => extracted_pack,
		Err(archive_error) => {
			error!("{archive_error}");

			return 4;
		}
	};

	let output_file_path = squash_options.global_options.output_file_path.clone();
	let start_instant = Instant::now();

	squash(squash_options, title_controller).map_or_else(
		|err| {
			log::debug!(
				target: debug_log::DEBUG_LOG_TARGET,
				"The pack processing operation failed. Error, with the errors that caused it: {}",
				debug_log::error_chain(&err)
			);

			error!("{}", describe_error(&err, true));

			128
		},
		|file_counts| {
			let process_time = start_instant.elapsed();

			debug!(
				"{} ({} pack files, {} pack files stored, {}.{:03} s)",
				output_file_path.metadata().ok().map_or_else(
					|| Cow::Borrowed("Pack processed"),
					|metadata| Cow::Owned(format!(
						"{} generated, {:.3} MiB",
						output_file_path.as_os_str().to_string_lossy(),
						metadata.len() as f64 / (1024.0 * 1024.0)
					))
				),
				file_counts.map_or_else(
					|| Cow::Borrowed("unknown"),
					|(total_file_count, _)| Cow::Owned(format!("{total_file_count}"))
				),
				file_counts.map_or_else(
					|| Cow::Borrowed("unknown"),
					|(_, processed_file_count)| Cow::Owned(format!("{processed_file_count}"))
				),
				process_time.as_secs(),
				process_time.subsec_millis()
			);

			0
		}
	)
}

/// Makes a path absolute, without needing it to exist and without the `\\?\` prefix of canonical Windows paths.
fn absolute_path(path: &std::path::Path) -> std::path::PathBuf {
	std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Starts the debug log, and writes what is known at the start of the run: the program, the machine, the
/// command line, the environment and the options.
fn start_debug_log(
	log_options: &fork_options::LogOptions,
	options_file_path: Option<&String>,
	options_string: &str,
	fork_options: &ForkOptions
) {
	let path = log_options.file.clone().unwrap_or_else(|| {
		options_file_path
			.and_then(|options_file_path| {
				std::path::Path::new(options_file_path)
					.parent()
					.map(|directory| directory.join(fork_options::DEFAULT_LOG_FILE_NAME))
			})
			.unwrap_or_else(|| fork_options::DEFAULT_LOG_FILE_NAME.into())
	});

	if let Err(error) = debug_log::start(&path) {
		warn!(
			"The debug log could not be started at {}: {error}. The program goes on without it",
			absolute_path(&path).display()
		);

		return;
	}

	debug_log::log_panics();

	info!("Writing a debug log to {}", absolute_path(&path).display());

	let target = debug_log::DEBUG_LOG_TARGET;
	let environment = |name: &str| match env::var_os(name) {
		Some(value) => format!("{}", value.to_string_lossy()),
		None => "(not set)".to_string()
	};

	log::debug!(
		target: target,
		"PackSquash debug log. Send this file when something does not work as it should.\n\
		Program: PackSquash {} ({}, {}) for {}\n\
		Started at: {}\n\
		Operating system: {} ({}), architecture {}\n\
		Hardware threads: {}\n\
		Command line: {:?}\n\
		Working directory: {}\n\
		Options file: {}\n\
		Temporary directory: {}\n\
		PACKSQUASH_LOG: {}. RUST_LOG: {}\n\
		PATH: {}",
		env!("PACKSQUASH_BUILD_VERSION"),
		env!("CARGO_PROFILE"),
		UtcDateTime::from_timespec(env!("PACKSQUASH_BUILD_TIMESTAMP").parse().unwrap(), 0)
			.map_or_else(|_| "unknown build date".to_string(), |date| {
				format!("built {:04}-{:02}-{:02}", date.year(), date.month(), date.month_day())
			}),
		env!("CARGO_TARGET_TRIPLE"),
		debug_log::format_time(std::time::SystemTime::now()),
		env::consts::OS,
		env::consts::FAMILY,
		env::consts::ARCH,
		std::thread::available_parallelism().map_or(0, std::num::NonZeroUsize::get),
		env::args().collect::<Vec<_>>(),
		env::current_dir().map_or_else(|error| format!("unknown ({error})"), |path| path.display().to_string()),
		options_file_path.map_or_else(
			|| "read from the standard input".to_string(),
			|path| format!("{} ({})", path, absolute_path(std::path::Path::new(path)).display())
		),
		env::temp_dir().display(),
		environment("PACKSQUASH_LOG"),
		environment("RUST_LOG"),
		environment("PATH")
	);

	debug_log::write_block("options file", options_string);

	log::debug!(
		target: target,
		"Options that only the command-line program reads: {fork_options:#?}"
	);
}

/// Describes a pack processing error for the user, with a hint for the likely fix. The links to the
/// documentation are left out when `with_links` is false, which is the case for every pack of a bulk
/// run, because the bulk run prints them once at the end.
fn describe_error(err: &PackSquasherError, with_links: bool) -> String {
	format!(
		"Pack processing error: {}{}{}{}",
		err,
		// We print both informational and error pack file status updates.
		// If the error was in one of those, hint the user at the status
		// update that contains the most information about the error
		if matches!(err, PackSquasherError::PackFileError) {
			"\nAnother error message with more information was emitted before. \
			You might need to scroll up to see it."
		} else {
			""
		},
		error_hints::hint(err)
			.map(|hint| format!("\nHow to fix: {hint}"))
			.unwrap_or_default(),
		if with_links {
			format!(
				"\nMore help for this fork: <{FORK_TROUBLESHOOTING_URL}>\n\
				Original PackSquash troubleshooting: <{UPSTREAM_TROUBLESHOOTING_URL}>\n\
				For more details, set the PACKSQUASH_LOG environment variable to debug and run \
				PackSquash again. In cmd: set PACKSQUASH_LOG=debug"
			)
		} else {
			String::new()
		}
	)
}

/// Parses the text of an options file. The options that only this program reads are split off
/// before the rest is read as the options of the library. Without those options, the text is
/// parsed directly, which gives error messages that point to the position of the error.
fn parse_options(options_string: &str) -> Result<(SquashOptions, ForkOptions, Table), String> {
	let mut table = options_string
		.parse::<Table>()
		.map_err(|error| error.to_string())?;

	if !has_fork_options(&table) {
		return toml::from_str::<SquashOptions>(options_string)
			.map(|squash_options| (squash_options, ForkOptions::default(), table))
			.map_err(|error| error.to_string());
	}

	let fork_options = split_fork_options(&mut table)?;

	// The bulk mode sets the pack directory of every pack by itself, but the options need one to be read
	let mut table_to_read = table.clone();

	if fork_options.bulk.is_some() {
		table_to_read
			.entry("pack_directory")
			.or_insert_with(|| Value::String(".".to_string()));
	}

	Value::Table(table_to_read)
		.try_into::<SquashOptions>()
		.map(|squash_options| (squash_options, fork_options, table))
		.map_err(|error| error.to_string())
}

/// Describes the packs of an archive for an error message.
fn describe_packs<'a>(roots: impl Iterator<Item = &'a str>) -> String {
	let names: Vec<_> = roots
		.map(|root| {
			if root.is_empty() {
				"(the root of the archive)".to_string()
			} else {
				root.to_string()
			}
		})
		.collect();

	if names.len() > 10 {
		format!("{}, and {} more", names[..10].join(", "), names.len() - 10)
	} else {
		names.join(", ")
	}
}

/// If the pack directory is a file, treats it as an archive, finds the pack inside, and extracts it into
/// a temporary directory. The pack directory option is changed to point to the extracted pack. The result
/// has to be kept until the squash operation ends, because the temporary directory is deleted when it is dropped.
/// Returns `None` if the pack directory is not a file.
fn prepare_archive_input(
	squash_options: &mut SquashOptions,
	fork_options: &ForkOptions
) -> Result<Option<ExtractedPacks>, String> {
	let archive_path = squash_options.pack_directory.clone();

	if !archive_path.is_file() {
		return Ok(None);
	}

	let display = archive_path.display();
	let depth = fork_options.archive_pack_search_depth;

	info!("{display} is a file. Looking for a pack inside it...");

	let scan = match scan_archive(&archive_path, depth) {
		Ok(Some(scan)) => scan,
		Ok(None) => {
			return Err(format!(
				"{display} is a file, but not an archive that PackSquash can read. \
				Set pack_directory to a directory, or to a ZIP, 7z, TAR (also .tar.gz, .tar.xz, .tar.zst and .tar.bz2) or RAR archive"
			));
		}
		Err(error) => {
			log::debug!(
				target: debug_log::DEBUG_LOG_TARGET,
				"The archive {display} could not be read. Error, with the errors that caused it: {}",
				debug_log::error_chain(&error)
			);

			return Err(format!("Couldn't read the archive {display}: {error}"));
		}
	};

	let several_packs_error = |roots: &[String]| {
		format!(
			"The archive {display} has {} packs: {}. PackSquash squashes one pack per run. \
			Set pack_directory to an archive or a directory that has a single pack, \
			or set bulk_input_directory to squash all of them",
			roots.len(),
			describe_packs(roots.iter().map(String::as_str))
		)
	};

	if let Some(roots) = &scan.pack_roots {
		match roots.len() {
			0 => {
				return Err(format!(
					"No pack was found in the archive {display}. A pack has a pack.mcmeta file and an \
					assets or data directory, or a manifest.json file of a Bedrock Edition resource pack. \
					It can be at most {depth} directories deep, which archive_pack_search_depth changes"
				));
			}
			1 => {}
			_ => return Err(several_packs_error(roots))
		}
	}

	info!(
		"Extracting the pack from the {} archive...",
		scan.format.name()
	);

	let extracted_packs = extract_packs(
		&archive_path,
		&scan,
		depth,
		ExtractLimits {
			max_bytes: fork_options.archive_max_extracted_bytes,
			..ExtractLimits::default()
		}
	)
	.map_err(|error| {
		log::debug!(
			target: debug_log::DEBUG_LOG_TARGET,
			"The pack could not be extracted from the archive {display}. Error, with the errors that caused it: {}",
			debug_log::error_chain(&error)
		);

		format!("Couldn't extract the pack from the archive {display}: {error}")
	})?;

	if extracted_packs.packs.len() != 1 {
		return Err(several_packs_error(
			&extracted_packs
				.packs
				.iter()
				.map(|(root, _)| root.clone())
				.collect::<Vec<_>>()
		));
	}

	squash_options.pack_directory = extracted_packs.packs[0].1.clone();

	Ok(Some(extracted_packs))
}

fn squash(
	squash_options: SquashOptions,
	mut title_controller: Option<TerminalTitleController>
) -> Result<Option<(u64, u64)>, PackSquasherError> {
	let (sender, mut receiver) = channel(64);

	// Move on to the "processing" title phase
	if let Some(title_controller) = &mut title_controller {
		title_controller.next_title_phase();
		title_controller.show();
	}

	// Build the runtime we're going to use to concurrently wait for status messages and update
	// the title on a single thread: updating the display
	let runtime = runtime::Builder::new_current_thread()
		.enable_time()
		.build()?;

	let cli_update_task = runtime.spawn(async move {
		/// The maximum interval of time between two progress ticks of the title. Used to assure
		/// the user that progress is being made even when something takes a while to optimize.
		const PROGRESS_TICK_INTERVAL: Duration = Duration::from_secs(1);

		let mut total_file_count = 0;
		let mut processed_file_count = 0;
		let progress_tick_timer = sleep(PROGRESS_TICK_INTERVAL);

		tokio::pin!(progress_tick_timer);

		loop {
			select! {
				// Give priority to status updates
				biased;

				status_update_message = receiver.recv() => match status_update_message {
					Some(status_update) => match status_update {
						PackSquasherStatus::PackFileProcessed(pack_file_status) => {
							log::debug!(
								target: debug_log::DEBUG_LOG_TARGET,
								"Result for the pack file {}: strategy {:?}, skipped {}, intentionally skipped {}, error {:?}",
								pack_file_status.path().as_str(),
								pack_file_status.optimization_strategy(),
								pack_file_status.skipped(),
								pack_file_status.intentionally_skipped(),
								pack_file_status.optimization_error()
							);

							total_file_count += 1;
							processed_file_count += 1 - u64::from(pack_file_status.skipped());

							match pack_file_status.optimization_error() {
								Some(error_description) => error!(
									"{}: {}",
									pack_file_status.path().as_str(),
									error_description
								),
								None => {
									// Files that the options ask to leave out are expected to be skipped, so they
									// should not be reported like the ones that may signal a problem
									if pack_file_status.skipped() && !pack_file_status.intentionally_skipped() {
										warn!(
											"{}: {}",
											pack_file_status.path().as_str(),
											pack_file_status.optimization_strategy()
										);
									} else {
										trace!(
											"{}: {}",
											pack_file_status.path().as_str(),
											pack_file_status.optimization_strategy()
										);
									};
								}
							};

							if let Some(title_controller) = &mut title_controller {
								title_controller.advance_and_show();

								// Prevent the forceful title progress tick from running too soon after this
								progress_tick_timer
									.as_mut()
									.reset(tokio::time::Instant::now() + PROGRESS_TICK_INTERVAL);
							}
						}
						PackSquasherStatus::ZipFinish => {
							info!("Finishing up ZIP file...");
							log::debug!(
								target: debug_log::DEBUG_LOG_TARGET,
								"All pack files were processed. Finishing the ZIP file: writing its central directory and the other structures"
							);

							// Move on to the "finishing" title phase
							if let Some(title_controller) = &mut title_controller {
								title_controller.next_title_phase();
								title_controller.show();
							}
						}
						PackSquasherStatus::Notice(notice) => info!("{notice}"),
						PackSquasherStatus::Warning(warning) => match warning {
							PackSquasherWarning::UnusablePreviousZip(err) => warn!(
								"The previous ZIP file could not be read. It will not be used to speed up processing. \
									Was the file last modified by PackSquash? Cause: {err}"
							),
							PackSquasherWarning::PredictableSystemTimeSanitizationKey => warn!(
								"Used predictable system IDs to build encryption keys. The dates embedded in the result ZIP file, \
									which reveal when it was generated, may be easier to decrypt. For more information \
									about the topic, check out <https://packsquash.aylas.org/links/Low-entropy-system-ID-help>"
							),
							PackSquasherWarning::VolatileSystemTimeSanitizationKey => warn!(
								"Used a volatile system IDs to build encryption keys. You maybe should not reuse the result ZIP file, \
									as unexpected results can occur after you use your device as usual. For more information \
									about the topic, check out <https://packsquash.aylas.org/links/Volatile-system-ID-help>"
							),
							#[cfg(unix)]
							PackSquasherWarning::ConcurrencyLimitedDueToOpenFdLimits => warn!(
								"The number of pack files that will be processed in parallel was reduced to avoid \
								exceeding open file descriptor limits. Please increase the open file descriptor \
								limit for optimum performance, or decrease the number of threads"),
							_ => unimplemented!()
						},
						_ => unimplemented!()
					}
					None => {
						// PackSquasher has finished its work, and it will not send any other messages
						break
					}
				},
				() = &mut progress_tick_timer => {
					// We have not yet received any message from the PackSquasher. Change the title
					// so that we give the user the illusion of some progress, and then schedule
					// another progress tick
					if let Some(title_controller) = &mut title_controller {
						title_controller.advance_and_show();

						progress_tick_timer
							.as_mut()
							.reset(tokio::time::Instant::now() + PROGRESS_TICK_INTERVAL);
					}
				}
			}
		}

		(total_file_count, processed_file_count)
	});

	// Squash the pack! This blocks until the operation is complete, so we can't run it in this thread
	let packsquasher = runtime
		.spawn_blocking(|| PackSquasher::new().run(OsFilesystem, squash_options, Some(sender)));

	runtime.block_on(async {
		// Wait for completion. Unwrap the handle because any panic in the thread is fatal anyway,
		// and we should propagate it
		match packsquasher.await.unwrap() {
			Ok(()) => Ok(cli_update_task.await.ok()),
			Err(err) => Err(err)
		}
	})
}

/// Prints `PackSquash` version information to the standard output stream.
fn print_version_information(verbose: bool) {
	let build_date_time =
		UtcDateTime::from_timespec(env!("PACKSQUASH_BUILD_TIMESTAMP").parse().unwrap(), 0).unwrap();

	println!(
		"PackSquash {} ({}, {}) for {}",
		env!("PACKSQUASH_BUILD_VERSION"),
		env!("CARGO_PROFILE"),
		format_args!(
			"{:04}-{:02}-{:02}",
			build_date_time.year(),
			build_date_time.month(),
			build_date_time.month_day()
		),
		env!("CARGO_TARGET_TRIPLE")
	);
	println!("{}", env!("CARGO_PKG_DESCRIPTION"));
	println!();

	if verbose {
		println!("Copyright (C) {}", env!("CARGO_PKG_AUTHORS"));
		println!();
		println!("This program is free software: you can redistribute it and/or modify");
		println!("it under the terms of the GNU Affero General Public License as");
		println!("published by the Free Software Foundation, either version 3 of the");
		println!("License, or (at your option) any later version.");
		println!();
		println!("This program is distributed free of charge in the hope that it will");
		println!("be useful, but WITHOUT ANY WARRANTY; without even the implied warranty");
		println!("of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the");
		println!("GNU Affero General Public License for more details.");
		println!();
		println!("You should have received a copy of the GNU Affero General Public License");
		println!("along with this program. If not, see <https://www.gnu.org/licenses/>.");
	} else {
		println!("This program comes with ABSOLUTELY NO WARRANTY.");
		println!("This is free software, and you are welcome to redistribute it");
		println!("under certain conditions. Use the -v command line switch for");
		println!("more details about these conditions.");
	}
}

/// Initializes the logging of the application, responsible for showing to the user relevant
/// application operation information.
fn init_logger(enable_emoji: bool, enable_colors: bool) {
	let mut logger_builder = Builder::new();

	logger_builder
		.target(LOG_TARGET)
		.write_style(if enable_colors {
			WriteStyle::Always
		} else {
			WriteStyle::Never
		})
		// Hide log messages from libraries by default
		.filter(Some("packsquash"), LevelFilter::max())
		.format(move |f, record| {
			use std::io::Write;

			let (level_color, level_icon, bold_effect) = match record.level() {
				Level::Error => (
					Color::Ansi(AnsiColor::Red),
					if enable_emoji { '❌' } else { '!' },
					Effects::BOLD
				),
				Level::Warn => (
					Color::Ansi(AnsiColor::Yellow),
					if enable_emoji { '⚡' } else { '*' },
					Effects::new()
				),
				Level::Info => (
					Color::Ansi(AnsiColor::Cyan),
					if enable_emoji { '🔷' } else { '-' },
					Effects::new()
				),
				Level::Debug => (
					Color::Ansi(AnsiColor::Green),
					if enable_emoji { '🍀' } else { '#' },
					Effects::new()
				),
				Level::Trace => (
					Color::Ansi(AnsiColor::White),
					if enable_emoji { '🏁' } else { '>' },
					Effects::new()
				)
			};

			let default_style = f.default_level_style(record.level());
			let style = default_style
				.fg_color(Some(level_color))
				.effects(bold_effect)
				.render();
			let reset_style = default_style.render_reset();

			writeln!(
				f,
				"{}",
				format!("{style}{} {}{reset_style}", level_icon, record.args())
					// Emojis are usually displayed with a non-monospaced font that is wider
					// than other characters. Output an extra space as a hack to look pretty
					.replace('\n', if enable_emoji { "\n   " } else { "\n  " })
			)
		});

	if let Ok(log_filters) = env::var("PACKSQUASH_LOG").or_else(|_| env::var("RUST_LOG")) {
		logger_builder.parse_filters(&log_filters);
	}

	// The console logger is put behind the logger that also writes the debug log file, if the options ask for it.
	// The maximum level is the highest one, because the detailed messages of the library and the debug log
	// need them, and every logger decides what it wants
	log::set_max_level(log::LevelFilter::Trace);

	if log::set_boxed_logger(Box::new(debug_log::TeeLogger::new(logger_builder.build()))).is_err() {
		eprintln!("The logger could not be started");
	}
}
