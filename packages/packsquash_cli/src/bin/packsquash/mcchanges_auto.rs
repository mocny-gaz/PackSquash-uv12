use std::{
	env, fs,
	path::{Path, PathBuf},
	process::{Command, Stdio}
};

use log::{info, warn};
use packsquash::config::SquashOptions;
use tempfile::NamedTempFile;
use toml::{Table, Value};

use crate::fork_options::ForkOptions;

const RELEASES_URL: &str = "https://github.com/mocny-gaz/mc-changes/releases/latest/download";

enum Download {
	Done(NamedTempFile),
	UpToDate,
	Failed
}

fn download(jar_name: &str, local: Option<&Path>) -> Download {
	let Ok(file) = tempfile::Builder::new()
		.prefix("packsquash-mcchanges-")
		.suffix(".jar")
		.tempfile()
	else {
		return Download::Failed;
	};

	let mut command = Command::new("curl");
	command.args([
		"--fail",
		"--silent",
		"--location",
		"--connect-timeout",
		"10",
		"--max-time",
		"900",
		"--remote-time"
	]);

	if let Some(local) = local {
		command.arg("--time-cond").arg(local);
	}

	let succeeded = command
		.arg("--output")
		.arg(file.path())
		.arg(format!("{RELEASES_URL}/{jar_name}"))
		.stdout(Stdio::null())
		.stderr(Stdio::null())
		.status()
		.is_ok_and(|status| status.success());

	match fs::metadata(file.path()) {
		Ok(metadata) if succeeded && metadata.len() > 0 => Download::Done(file),
		Ok(_) if succeeded => Download::UpToDate,
		_ => Download::Failed
	}
}

fn is_bedrock(table: &Table, input: &Path) -> bool {
	table.get("pack_edition").and_then(Value::as_str) == Some("bedrock")
		|| (input.join("manifest.json").is_file() && !input.join("pack.mcmeta").exists())
}

pub fn prepare(
	fork_options: &ForkOptions,
	squash_options: &mut SquashOptions,
	table: &mut Table
) -> Option<NamedTempFile> {
	if !fork_options.mcchanges_download || squash_options.global_options.mcchanges_path.is_some() {
		return None;
	}

	let input = fork_options
		.bulk
		.as_ref()
		.map_or(squash_options.pack_directory.as_path(), |bulk| {
			bulk.input_directory.as_path()
		});
	let output = match &fork_options.bulk {
		Some(bulk) => bulk.output_directory.clone(),
		None => squash_options
			.global_options
			.output_file_path
			.parent()
			.map(Path::to_path_buf)
	};

	let mut folders: Vec<PathBuf> = env::current_exe()
		.ok()
		.and_then(|exe| exe.parent().map(Path::to_path_buf))
		.into_iter()
		.collect();
	folders.push(if input.is_file() {
		input.parent().map(Path::to_path_buf).unwrap_or_default()
	} else {
		input.to_path_buf()
	});
	folders.extend(output);

	let jar_name = if is_bedrock(table, input) {
		"mcchanges-bedrock.jar"
	} else {
		"mcchanges-java.jar"
	};
	let local = folders
		.iter()
		.map(|folder| folder.join(jar_name))
		.find(|path| path.is_file());

	let mut downloaded = None;

	if !fork_options.mcchanges_nowifi {
		match download(jar_name, local.as_deref()) {
			Download::Done(file) => {
				info!("Downloaded the newest {jar_name}. PackSquash deletes it when it ends");
				downloaded = Some(file);
			}
			Download::UpToDate => info!("The {jar_name} that was found is the newest"),
			Download::Failed => warn!(
				"Couldn't download {jar_name}. Is there an internet connection, and is curl installed? Set mcchanges_nowifi to true to skip trying"
			)
		}
	}

	let path = downloaded
		.as_ref()
		.map(|file| file.path().to_path_buf())
		.or(local);

	match path {
		Some(path) => {
			info!("Using {}", path.display());
			table.insert(
				"mcchanges_path".into(),
				Value::String(path.to_string_lossy().into_owned())
			);
			squash_options.global_options.mcchanges_path = Some(path);
		}
		None => warn!(
			"There is no {jar_name} next to PackSquash, in the input folder or in the output folder, and it was not downloaded, so the files that the game already has are not skipped"
		)
	}

	downloaded
}

#[cfg(test)]
mod tests {
	use super::*;

	fn options(directory: &Path, extra: &str) -> (SquashOptions, ForkOptions, Table) {
		crate::parse_options(&format!(
			"pack_directory = '{0}'\noutput_file_path = '{0}/out.zip'\n{extra}",
			directory.display()
		))
		.unwrap()
	}

	#[test]
	fn a_jar_in_the_input_folder_is_used_without_the_network() {
		let directory = tempfile::tempdir().unwrap();
		let jar = directory.path().join("mcchanges-java.jar");

		fs::write(&jar, b"jar").unwrap();

		let (mut squash_options, fork_options, mut table) =
			options(directory.path(), "mcchanges_nowifi = true\n");

		assert!(prepare(&fork_options, &mut squash_options, &mut table).is_none());
		assert_eq!(
			squash_options.global_options.mcchanges_path,
			Some(jar.clone())
		);
		assert_eq!(
			table.get("mcchanges_path").and_then(Value::as_str),
			jar.to_str()
		);
	}

	#[test]
	fn a_bedrock_pack_looks_for_the_bedrock_jar() {
		let directory = tempfile::tempdir().unwrap();

		fs::write(directory.path().join("manifest.json"), b"{}").unwrap();
		fs::write(directory.path().join("mcchanges-java.jar"), b"java").unwrap();
		fs::write(directory.path().join("mcchanges-bedrock.jar"), b"bedrock").unwrap();

		let (mut squash_options, fork_options, mut table) =
			options(directory.path(), "mcchanges_nowifi = true\n");

		prepare(&fork_options, &mut squash_options, &mut table);

		assert_eq!(
			squash_options.global_options.mcchanges_path,
			Some(directory.path().join("mcchanges-bedrock.jar"))
		);
	}

	#[test]
	fn nothing_changes_without_a_jar_or_when_the_user_decides() {
		let directory = tempfile::tempdir().unwrap();

		let (mut squash_options, fork_options, mut table) =
			options(directory.path(), "mcchanges_nowifi = true\n");

		assert!(prepare(&fork_options, &mut squash_options, &mut table).is_none());
		assert_eq!(squash_options.global_options.mcchanges_path, None);
		assert!(!table.contains_key("mcchanges_path"));

		fs::write(directory.path().join("mcchanges-java.jar"), b"jar").unwrap();

		let (mut squash_options, fork_options, mut table) =
			options(directory.path(), "mcchanges_download = false\n");

		assert!(prepare(&fork_options, &mut squash_options, &mut table).is_none());
		assert_eq!(squash_options.global_options.mcchanges_path, None);

		let (mut squash_options, fork_options, mut table) = options(
			directory.path(),
			"mcchanges_nowifi = true\nmcchanges_path = 'mine.jar'\n"
		);

		prepare(&fork_options, &mut squash_options, &mut table);

		assert_eq!(
			squash_options.global_options.mcchanges_path,
			Some(PathBuf::from("mine.jar")),
			"the path of the user wins"
		);
	}
}
