//! A switch to pause the processing of pack files, used by the window of the program.

use std::sync::atomic::{AtomicBool, Ordering};

static REQUESTED: AtomicBool = AtomicBool::new(false);
static IDLE: AtomicBool = AtomicBool::new(false);

/// Asks the squashers of this process to stop taking new pack files (`true`), or to go on
/// (`false`). The files in progress finish first, so the output is never cut in the middle of a
/// file, and a paused squash gives the same output as one that was never paused.
pub fn request(paused: bool) {
	REQUESTED.store(paused, Ordering::Release);
}

pub(crate) fn requested() -> bool {
	REQUESTED.load(Ordering::Acquire)
}

/// Tells if a squasher waits because of a pause request, with no pack file in progress.
pub fn is_idle() -> bool {
	IDLE.load(Ordering::Acquire)
}

pub(crate) fn set_idle(idle: bool) {
	IDLE.store(idle, Ordering::Release);
}

#[cfg(test)]
mod tests {
	use std::{
		path::Path,
		thread,
		time::{Duration, Instant}
	};

	use super::*;
	use crate::{
		PackSquasher, PackSquasherStatus,
		config::{GlobalOptions, SquashOptions},
		mcchanges::{
			ZipArchive,
			tests::{temp_dir, write_file}
		},
		vfs::os_fs::OsFilesystem
	};

	fn pack(dir: &Path) {
		write_file(
			dir,
			"pack.mcmeta",
			br#"{"pack":{"pack_format":34,"description":"x"}}"#
		);

		for index in 0..80 {
			write_file(
				dir,
				&format!("assets/minecraft/lang/l{index}.json"),
				format!("{{\"key.{index}\": \"value {index}\"}}").as_bytes()
			);
		}
	}

	fn options(dir: &Path, output: &Path) -> SquashOptions {
		let mut options = SquashOptions {
			pack_directory: dir.to_path_buf(),
			global_options: GlobalOptions {
				output_file_path: output.to_path_buf(),
				..GlobalOptions::default()
			},
			file_options: Default::default()
		};

		options.global_options.zip_compression_iterations = 1;
		options.global_options.never_store_squash_times = true;
		options
	}

	#[test]
	fn a_pause_before_the_first_file_holds_every_file_back() {
		let (dir, output) = (temp_dir(), temp_dir());
		let output = output.path().join("pack.zip");

		pack(dir.path());

		let options = options(dir.path(), &output);
		let (sender, mut receiver) = tokio::sync::mpsc::channel(4096);

		request(true);

		let worker =
			thread::spawn(move || PackSquasher::new().run(OsFilesystem, options, Some(sender)));
		let started = Instant::now();

		while !is_idle() && started.elapsed() < Duration::from_secs(20) {
			thread::sleep(Duration::from_millis(10));
		}

		assert!(is_idle());
		thread::sleep(Duration::from_millis(200));

		let mut processed = 0;

		while let Ok(status) = receiver.try_recv() {
			if matches!(status, PackSquasherStatus::PackFileProcessed(_)) {
				processed += 1;
			}
		}

		request(false);

		assert_eq!(processed, 0);
		assert!(worker.join().unwrap().is_ok());
		assert!(output.exists());
	}

	#[test]
	fn pausing_and_resuming_all_the_time_does_not_change_the_files_in_the_zip() {
		let (dir, output) = (temp_dir(), temp_dir());

		pack(dir.path());

		let mut zips = Vec::new();

		for toggle in [false, true, true] {
			let output = output.path().join(format!("pack{}.zip", zips.len()));
			let options = options(dir.path(), &output);
			let (sender, _receiver) = tokio::sync::mpsc::channel(4096);
			let worker =
				thread::spawn(move || PackSquasher::new().run(OsFilesystem, options, Some(sender)));

			while toggle && !worker.is_finished() {
				request(true);
				thread::sleep(Duration::from_millis(1));
				request(false);
				thread::sleep(Duration::from_millis(1));
			}

			request(false);
			assert!(worker.join().unwrap().is_ok());
			zips.push(ZipArchive::parse(std::fs::read(output).unwrap()).unwrap());
		}

		let mut names: Vec<String> = (0..80)
			.map(|index| format!("assets/minecraft/lang/l{index}.json"))
			.collect();

		names.push("pack.mcmeta".to_string());

		for name in &names {
			let expected = zips[0].read(name);

			assert!(expected.is_some(), "{name} should be in the ZIP");
			assert!(
				zips.iter().all(|zip| zip.read(name) == expected),
				"{name} changed"
			);
		}
	}
}
