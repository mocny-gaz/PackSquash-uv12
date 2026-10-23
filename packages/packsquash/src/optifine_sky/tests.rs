use std::fs;

use flate2::Compression;

use super::*;
use crate::baseline_pack::ZipArchive;
use crate::baseline_pack::tests::{png, temp_dir, write_file};
use crate::config::{GlobalOptions, MinecraftMod, SquashOptions};
use crate::vfs::os_fs::OsFilesystem;
use crate::{PackSquasher, PackSquasherStatus};

const SKY_ENTRY: &[u8] = b"source=./cloud.png\nstartFadeIn=6:00\nendFadeIn=7:00\n";

async fn detector(pack_dir: &Path) -> UnusedSkyImageDetector {
	UnusedSkyImageDetector::scan(&OsFilesystem, pack_dir, IteratorTraversalOptions::default()).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn images_not_named_by_any_entry_are_unused() {
	let pack_dir = temp_dir();
	write_file(
		pack_dir.path(),
		"assets/minecraft/optifine/sky/world0/sky1.properties",
		SKY_ENTRY
	);

	let detector = detector(pack_dir.path()).await;

	assert!(!detector.is_unused("assets/minecraft/optifine/sky/world0/cloud.png"));
	assert!(detector.is_unused("assets/minecraft/optifine/sky/world0/leftover.png"));
	// Only sky images are considered
	assert!(!detector.is_unused("assets/minecraft/optifine/colormap/leftover.png"));
	assert!(!detector.is_unused("assets/minecraft/optifine/sky/world0/sky1.properties"));
	assert!(!detector.is_unused("assets/minecraft/textures/blocks/leftover.png"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn nothing_is_unused_without_sky_entries() {
	let pack_dir = temp_dir();
	write_file(
		pack_dir.path(),
		"assets/minecraft/optifine/sky/world0/leftover.png",
		b""
	);

	assert!(
		!detector(pack_dir.path())
			.await
			.is_unused("assets/minecraft/optifine/sky/world0/leftover.png")
	);
}

/// Not async because `PackSquasher::run` starts its own runtime.
#[test]
fn squashing_leaves_out_unused_sky_images_only_if_asked() {
	let sky_image = png(1, 1, &[1, 2, 3, 255], Compression::best());

	for (skip_unused_images, allow_optifine) in [(true, true), (false, true), (true, false)] {
		let (pack_dir, output_dir) = (temp_dir(), temp_dir());

		write_file(
			pack_dir.path(),
			"pack.mcmeta",
			br#"{"pack": {"pack_format": 1, "description": "Test pack"}}"#
		);
		write_file(
			pack_dir.path(),
			"assets/minecraft/optifine/sky/world0/sky1.properties",
			SKY_ENTRY
		);
		write_file(
			pack_dir.path(),
			"assets/minecraft/optifine/sky/world0/cloud.png",
			&sky_image
		);
		write_file(
			pack_dir.path(),
			"assets/minecraft/optifine/sky/world0/leftover.png",
			&sky_image
		);

		let output_path = output_dir.path().join("pack.zip");
		let mut global_options = GlobalOptions {
			skip_unused_optifine_sky_images: skip_unused_images,
			output_file_path: output_path.clone(),
			..GlobalOptions::default()
		};
		global_options.zip_compression_iterations = 1;
		if allow_optifine {
			global_options.allow_mods = MinecraftMod::Optifine.into();
		}

		let (sender, mut receiver) = tokio::sync::mpsc::channel(64);

		PackSquasher::new()
			.run(
				OsFilesystem,
				SquashOptions {
					pack_directory: pack_dir.path().to_path_buf(),
					global_options,
					file_options: Default::default()
				},
				Some(sender)
			)
			.expect("Squashing a valid pack should succeed");

		let mut intentionally_skipped = Vec::new();
		while let Ok(status) = receiver.try_recv() {
			if let PackSquasherStatus::PackFileProcessed(status) = status {
				assert!(status.optimization_error().is_none());

				if status.intentionally_skipped() {
					intentionally_skipped.push(status.path().as_str().to_owned());
				}
			}
		}

		let output = ZipArchive::parse(fs::read(output_path).unwrap()).unwrap();
		let sky_directory = "assets/minecraft/optifine/sky/world0/";

		if skip_unused_images && allow_optifine {
			assert_eq!(
				intentionally_skipped,
				[format!("{sky_directory}leftover.png")]
			);
			assert!(!output.contains(&format!("{sky_directory}leftover.png")));
		} else {
			assert!(intentionally_skipped.is_empty());
			// Without OptiFine support, the whole sky directory is not included anyway,
			// so only check the result if OptiFine support was enabled
			assert_eq!(
				output.contains(&format!("{sky_directory}leftover.png")),
				allow_optifine
			);
		}

		if allow_optifine {
			assert!(output.contains(&format!("{sky_directory}cloud.png")));
			assert!(output.contains(&format!("{sky_directory}sky1.properties")));
		}
	}
}
