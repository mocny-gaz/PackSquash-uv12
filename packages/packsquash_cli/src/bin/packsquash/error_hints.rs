//! Short hints that tell the user how to fix the most likely causes of a pack processing error. The
//! hints are printed after the error, by default, without any logging option.

use packsquash::PackSquasherError;

/// The page of the fork documentation that lists the common problems and their fixes.
pub const FORK_TROUBLESHOOTING_URL: &str =
	"https://github.com/mocny-gaz/PackSquash-uv12/blob/master/docs/fork/USAGE.md#common-problems";

/// The page of the original PackSquash documentation about pack processing errors.
pub const UPSTREAM_TROUBLESHOOTING_URL: &str =
	"https://packsquash.aylas.org/links/Troubleshooting-pack-processing-errors";

/// Returns the hint for the given error, if there is a likely fix to point to.
pub fn hint(error: &PackSquasherError) -> Option<&'static str> {
	match error {
		PackSquasherError::McChangesError(_) => Some(
			"Check mcchanges_path and mcchanges_version in your options file. For a jar from \
			mc-changes, like mcchanges-java.jar, set mcchanges_path to the jar file, or to a folder \
			that has the jar with exactly that name, and set mcchanges_version to a game version \
			that the jar has, or leave it out to let PackSquash choose. For a game jar or a folder \
			of game files, set only mcchanges_path."
		),
		PackSquasherError::BedrockManifestError(_) => Some(
			"A Bedrock Edition pack needs a valid manifest.json at its root. If this is a Java \
			Edition pack, set pack_edition = 'java' in your options file."
		),
		PackSquasherError::PackMetadataError(_) => Some(
			"A Java Edition pack needs a valid pack.mcmeta file at its root. If this is a Bedrock \
			Edition pack, set pack_edition = 'bedrock' in your options file."
		),
		PackSquasherError::InvalidFileType(_) => Some(
			"pack_directory must be a directory or an archive, and output_file_path must be a \
			file path, not a directory."
		),
		PackSquasherError::InvalidFileGlobPattern(_) => Some(
			"A table name in your options file is not a valid glob pattern. Write it in quotes, \
			like ['assets/minecraft/textures/**/*.png']."
		),
		PackSquasherError::IoError(_) | PackSquasherError::SquashZip(_) => Some(
			"Check that the paths in your options file exist, that PackSquash can read the pack, \
			and that it can write output_file_path. The output file can not be open in another \
			program."
		),
		PackSquasherError::InvalidOptions(_) => {
			Some("Check the options in your options file against the Options files wiki page.")
		}
		_ => None
	}
}

#[cfg(test)]
mod tests {
	use std::io;

	use super::*;

	#[test]
	fn hints_are_given_for_the_errors_that_users_can_fix() {
		assert!(
			hint(&PackSquasherError::IoError(io::Error::other("x")))
				.is_some_and(|hint| hint.contains("output_file_path"))
		);
		assert!(
			hint(&PackSquasherError::InvalidFileType("x"))
				.is_some_and(|hint| hint.contains("pack_directory"))
		);
		assert!(
			hint(&PackSquasherError::InvalidOptions("x"))
				.is_some_and(|hint| hint.contains("Options files"))
		);
	}

	#[test]
	fn errors_with_an_earlier_message_have_no_hint() {
		assert_eq!(hint(&PackSquasherError::PackFileError), None);
	}
}
