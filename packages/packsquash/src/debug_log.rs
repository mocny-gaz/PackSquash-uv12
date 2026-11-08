//! Support for the detailed debug log.
//!
//! The library does not write the log. It sends detailed messages to the [`log`] crate with the target
//! [`TARGET`], which the command-line program only writes to a file when the user asks for a debug log, and
//! which is not printed in the console. The messages tell what PackSquash decided and why, with the values that
//! it used, so that a problem can be understood from the log alone.

/// The log target of the detailed messages. It does not start with `packsquash`, so that the console, which
/// shows the messages of that target, does not show these.
pub const TARGET: &str = "debuglog";

/// Tells whether detailed messages are wanted. Check it before doing any work that only a message needs.
pub(crate) fn enabled() -> bool {
	log::log_enabled!(target: TARGET, log::Level::Debug)
}

/// Writes a detailed message, if they are wanted. The arguments are only evaluated in that case.
macro_rules! detail {
	($($argument:tt)+) => {
		if $crate::debug_log::enabled() {
			log::debug!(target: $crate::debug_log::TARGET, $($argument)+);
		}
	};
}

/// Writes a detailed message about something that failed, if detailed messages are wanted.
macro_rules! detail_error {
	($($argument:tt)+) => {
		if $crate::debug_log::enabled() {
			log::error!(target: $crate::debug_log::TARGET, $($argument)+);
		}
	};
}

pub(crate) use {detail, detail_error};

/// Describes an error with the errors that caused it, from the outermost to the innermost, and with its
/// debug representation, which has the details that the user friendly message leaves out, like the
/// kind of an I/O error and the code of the operating system.
pub fn error_chain(error: &(dyn std::error::Error + 'static)) -> String {
	use std::fmt::Write as _;

	let mut description = format!("{error} [{error:?}]");
	let mut source = error.source();
	let mut depth = 1;

	while let Some(cause) = source {
		let _ = write!(
			description,
			"\n{}caused by: {cause} [{cause:?}]",
			"  ".repeat(depth)
		);
		source = cause.source();
		depth += 1;
	}

	description
}

#[cfg(test)]
mod tests {
	use std::{error::Error, fmt, io};

	use super::*;

	#[derive(Debug)]
	struct Outer(io::Error);

	impl fmt::Display for Outer {
		fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
			write!(f, "could not do the thing")
		}
	}

	impl Error for Outer {
		fn source(&self) -> Option<&(dyn Error + 'static)> {
			Some(&self.0)
		}
	}

	#[test]
	fn the_chain_of_an_error_has_every_level_with_its_details() {
		let error = Outer(io::Error::new(
			io::ErrorKind::PermissionDenied,
			"access denied"
		));
		let chain = error_chain(&error);
		let lines = chain.lines().collect::<Vec<_>>();

		assert_eq!(lines.len(), 2, "{chain}");
		assert!(
			lines[0].starts_with("could not do the thing [Outer("),
			"{chain}"
		);
		assert!(
			lines[1].starts_with("  caused by: access denied ["),
			"{chain}"
		);
		assert!(lines[1].contains("PermissionDenied"), "{chain}");
	}

	#[test]
	fn an_error_without_a_cause_is_one_line() {
		let chain = error_chain(&io::Error::from(io::ErrorKind::NotFound));

		assert_eq!(chain.lines().count(), 1, "{chain}");
		assert!(chain.contains("NotFound"), "{chain}");
	}
}
