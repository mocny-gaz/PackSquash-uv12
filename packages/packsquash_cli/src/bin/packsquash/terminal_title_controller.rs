use std::{
	sync::{Arc, Condvar, Mutex, PoisonError, mpsc},
	thread::{self, JoinHandle},
	time::{Duration, Instant}
};

use crate::terminal_title_setter::{
	TerminalTitleSetter, TerminalTitleSetterTrait, TerminalTitleString
};

/// The name that the terminal title starts with.
const TITLE_NAME: &str = "PackSquash-uv12";

/// How often the title is refreshed while it changes (the spinner and the CPU use).
const REFRESH: Duration = Duration::from_millis(500);

/// Controller for the title to be shown in the terminal or console attached to this process.
/// There are several title phases, each corresponding to one phase in the application lifecycle.
///
/// The title has the phase, the CPU use of this program and the name of the input that is being
/// squashed (the pack or the bulk folder, never a file, so it does not change all the time). A thread
/// refreshes it, so the CPU use stays up to date even when the program prints nothing.
pub struct TerminalTitleController {
	shared: Arc<(Mutex<TitleState>, Condvar)>,
	thread: Option<JoinHandle<()>>
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
	Idle,
	Processing,
	Finishing
}

struct TitleState {
	phase: Phase,
	input: Option<String>,
	stop: bool
}

impl TerminalTitleController {
	/// Creates a new terminal title controller, if it is possible to change the terminal or console
	/// title. It may not be possible to change the title if the process does not have any associated
	/// terminal or console, for example.
	///
	/// No title will be shown after creating the title controller. The callee is responsible for
	/// calling [`TerminalTitleController::show`] if it is desired to show the first title of the first
	/// phase.
	pub fn new() -> Option<Self> {
		let shared = Arc::new((
			Mutex::new(TitleState {
				phase: Phase::Idle,
				input: None,
				stop: false
			}),
			Condvar::new()
		));
		let (ready_sender, ready_receiver) = mpsc::channel();
		let thread_shared = Arc::clone(&shared);
		let thread = thread::Builder::new()
			.name("terminal title".into())
			.spawn(move || {
				// The setter is created in this thread, so it does not have to be sent to it
				let Some(title_setter) = TerminalTitleSetter::init() else {
					let _ = ready_sender.send(false);
					return;
				};

				let _ = ready_sender.send(true);
				update_titles(&title_setter, &thread_shared);
			})
			.ok()?;

		if ready_receiver.recv().unwrap_or(false) {
			Some(Self {
				shared,
				thread: Some(thread)
			})
		} else {
			let _ = thread.join();
			None
		}
	}

	fn change(&self, change: impl FnOnce(&mut TitleState)) {
		let (state, wake) = &*self.shared;

		change(&mut state.lock().unwrap_or_else(PoisonError::into_inner));
		wake.notify_all();
	}

	/// Shows the current title for the current title phase.
	pub fn show(&self) {
		self.change(|_| ());
	}

	/// The title refreshes itself, so this only makes sure that it is shown now.
	pub fn advance_and_show(&mut self) {
		self.show();
	}

	/// Sets the name of the input that is being squashed, like `bulk: my packs`.
	pub fn set_input(&mut self, input: &str) {
		self.change(|state| state.input = Some(input.to_string()));
	}

	/// Moves on to the next title phase, and shows its title.
	pub fn next_title_phase(&mut self) {
		self.change(|state| {
			state.phase = match state.phase {
				Phase::Idle => Phase::Processing,
				Phase::Processing | Phase::Finishing => Phase::Finishing
			}
		});
	}
}

impl Drop for TerminalTitleController {
	fn drop(&mut self) {
		self.change(|state| state.stop = true);

		if let Some(thread) = self.thread.take() {
			let _ = thread.join();
		}
	}
}

/// Sets the title again whenever it is different, until the controller is dropped.
fn update_titles(title_setter: &TerminalTitleSetter, shared: &(Mutex<TitleState>, Condvar)) {
	let (state, wake) = shared;
	let mut meter = CpuMeter::new();
	let mut spinner = 0_usize;
	let mut shown = String::new();
	let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);

	loop {
		let stop = guard.stop;
		let phase = if stop { Phase::Idle } else { guard.phase };
		let input = guard.input.clone();

		drop(guard);

		let title = title_text(phase, spinner, meter.percent(), input.as_deref());

		if title != shown {
			title_setter.set_title(&TerminalTitleString::from(title.as_str()));
			shown = title;
		}

		if stop {
			return;
		}

		spinner = spinner.wrapping_add(1);
		guard = wake
			.wait_timeout(
				state.lock().unwrap_or_else(PoisonError::into_inner),
				REFRESH
			)
			.unwrap_or_else(PoisonError::into_inner)
			.0;
	}
}

fn title_text(phase: Phase, spinner: usize, cpu: Option<u32>, input: Option<&str>) -> String {
	let mut title = match phase {
		Phase::Idle => return TITLE_NAME.to_string(),
		Phase::Processing => format!(
			"{TITLE_NAME} - Optimizing [{}]",
			['-', '\\', '|', '/'][spinner % 4]
		),
		Phase::Finishing => format!("{TITLE_NAME} - Finishing [{}]", [' ', '·'][spinner % 2])
	};

	if let Some(cpu) = cpu {
		title.push_str(&format!(" | CPU {cpu}%"));
	}

	if let Some(input) = input {
		title.push_str(&format!(" | {input}"));
	}

	title
}

/// Measures how much of the CPU this program uses, as Task Manager does: the CPU time of the
/// program divided by the time that passed and by the number of logical processors.
struct CpuMeter {
	last_time: Option<Duration>,
	last_at: Instant,
	logical_processors: f64,
	percent: Option<u32>
}

impl CpuMeter {
	fn new() -> Self {
		Self {
			last_time: process_cpu_time(),
			last_at: Instant::now(),
			logical_processors: thread::available_parallelism()
				.map_or(1.0, |count| count.get() as f64),
			percent: None
		}
	}

	/// The CPU use since the last measure, which is renewed once a second. It is nothing on the
	/// systems where the CPU time of the program is not known.
	fn percent(&mut self) -> Option<u32> {
		let elapsed = self.last_at.elapsed();

		if elapsed >= Duration::from_secs(1)
			&& let (Some(before), Some(now)) = (self.last_time, process_cpu_time())
		{
			let used = now.saturating_sub(before).as_secs_f64();

			self.percent = Some(
				((used / elapsed.as_secs_f64() / self.logical_processors) * 100.0)
					.round()
					.clamp(0.0, 100.0) as u32
			);
			self.last_time = Some(now);
			self.last_at = Instant::now();
		}

		self.percent
	}
}

#[cfg(windows)]
fn process_cpu_time() -> Option<Duration> {
	use windows_sys::Win32::{
		Foundation::FILETIME,
		System::Threading::{GetCurrentProcess, GetProcessTimes}
	};

	let zero = FILETIME {
		dwLowDateTime: 0,
		dwHighDateTime: 0
	};
	let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);

	// SAFETY: the pointers are to four values that live until the call ends, and the handle of
	// the current process is always valid
	#[allow(unsafe_code)]
	let worked = unsafe {
		GetProcessTimes(
			GetCurrentProcess(),
			&raw mut creation,
			&raw mut exit,
			&raw mut kernel,
			&raw mut user
		)
	};
	let hundreds_of_nanoseconds =
		|time: FILETIME| (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime);

	(worked != 0).then(|| {
		Duration::from_nanos((hundreds_of_nanoseconds(kernel) + hundreds_of_nanoseconds(user)) * 100)
	})
}

#[cfg(target_os = "linux")]
fn process_cpu_time() -> Option<Duration> {
	// The fields after the name of the program, which is in parentheses and may have spaces
	let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
	let fields = stat
		.rsplit_once(')')?
		.1
		.split_whitespace()
		.collect::<Vec<_>>();
	let ticks = fields.get(11)?.parse::<u64>().ok()? + fields.get(12)?.parse::<u64>().ok()?;

	// A tick is 1/100 of a second on every Linux system that matters
	Some(Duration::from_millis(ticks * 10))
}

#[cfg(not(any(windows, target_os = "linux")))]
fn process_cpu_time() -> Option<Duration> {
	None
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn the_title_has_the_phase_the_cpu_and_the_input() {
		assert_eq!(
			title_text(Phase::Idle, 0, Some(5), Some("x")),
			"PackSquash-uv12"
		);
		assert_eq!(
			title_text(Phase::Processing, 1, Some(37), Some("bulk: packs")),
			"PackSquash-uv12 - Optimizing [\\] | CPU 37% | bulk: packs"
		);
		assert_eq!(
			title_text(Phase::Finishing, 0, None, None),
			"PackSquash-uv12 - Finishing [ ]"
		);
	}
}
