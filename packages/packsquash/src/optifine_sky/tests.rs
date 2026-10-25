use std::fs;

use flate2::Compression;

use super::*;
use crate::baseline_pack::ZipArchive;
use crate::baseline_pack::tests::{png, temp_dir, write_file};
use crate::config::{GlobalOptions, MinecraftMod, SquashOptions};
use crate::vfs::os_fs::OsFilesystem;
use crate::{PackSquasher, PackSquasherStatus};

const SKY: &str = "assets/minecraft/optifine/sky/";
const SKY_ENTRY: &[u8] = b"source=./cloud.png\nstartFadeIn=6:00\nendFadeIn=7:00\n";

async fn optimizer(pack_dir: &Path, merge_identical_images: bool) -> SkyImageOptimizer {
	SkyImageOptimizer::scan(
		&OsFilesystem,
		pack_dir,
		IteratorTraversalOptions::default(),
		true,
		merge_identical_images
	)
	.await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn images_not_named_by_any_entry_are_unused() {
	let pack_dir = temp_dir();
	write_file(
		pack_dir.path(),
		&format!("{SKY}world0/sky1.properties"),
		SKY_ENTRY
	);

	let optimizer = optimizer(pack_dir.path(), false).await;

	assert_eq!(optimizer.skip_reason(&format!("{SKY}world0/cloud.png")), None);
	assert_eq!(
		optimizer.skip_reason(&format!("{SKY}world0/leftover.png")),
		Some("Unused by OptiFine custom sky entries")
	);
	// Only sky images are considered
	for path in [
		"assets/minecraft/optifine/colormap/leftover.png".to_owned(),
		format!("{SKY}world0/sky1.properties"),
		"assets/minecraft/textures/blocks/leftover.png".to_owned()
	] {
		assert_eq!(optimizer.skip_reason(&path), None, "{path}");
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn nothing_is_unused_without_sky_entries() {
	let pack_dir = temp_dir();
	write_file(pack_dir.path(), &format!("{SKY}world0/leftover.png"), b"");

	assert_eq!(
		optimizer(pack_dir.path(), true)
			.await
			.skip_reason(&format!("{SKY}world0/leftover.png")),
		None
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn identical_images_of_a_directory_are_merged() {
	let pack_dir = temp_dir();
	let p = pack_dir.path();

	write_file(p, &format!("{SKY}world0/sky1.properties"), b"source=./b.png\nblend=add\n");
	write_file(p, &format!("{SKY}world0/sky2.properties"), b"source = a.png\r\n");
	write_file(p, &format!("{SKY}world0/sky3.properties"), b"source=c.png\n");
	for (name, data) in [("a", b"same"), ("b", b"same"), ("c", b"diff")] {
		write_file(p, &format!("{SKY}world0/{name}.png"), data);
	}
	// The same bytes in another directory are a different image for the entries there
	write_file(p, &format!("{SKY}world1/sky.properties"), b"source=a.png\n");
	write_file(p, &format!("{SKY}world1/a.png"), b"same");

	let optimizer = optimizer(p, true).await;

	assert_eq!(
		optimizer.skip_reason(&format!("{SKY}world0/b.png")),
		Some("Merged with an identical OptiFine custom sky image")
	);
	for kept in ["world0/a.png", "world0/c.png", "world1/a.png"] {
		assert_eq!(optimizer.skip_reason(&format!("{SKY}{kept}")), None, "{kept}");
	}

	assert_eq!(
		optimizer
			.rewritten_entry(&format!("{SKY}world0/sky1.properties"))
			.as_deref(),
		Some(&b"source=./a.png\nblend=add\n"[..])
	);
	// Entries that do not mention merged images are left alone
	for untouched in ["world0/sky2.properties", "world0/sky3.properties", "world1/sky.properties"] {
		assert!(optimizer.rewritten_entry(&format!("{SKY}{untouched}")).is_none());
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn images_mentioned_from_other_directories_are_not_merged() {
	let pack_dir = temp_dir();
	let p = pack_dir.path();

	write_file(p, &format!("{SKY}world0/sky.properties"), b"source=a.png\n");
	write_file(p, &format!("{SKY}world0/a.png"), b"same");
	write_file(p, &format!("{SKY}world0/b.png"), b"same");
	// This entry may mean its own b.png, or the one of world0
	write_file(p, &format!("{SKY}world1/sky.properties"), b"source=../world0/b.png\n");

	let optimizer = optimizer(p, true).await;

	assert_eq!(optimizer.skip_reason(&format!("{SKY}world0/b.png")), None);
	assert!(optimizer.rewritten_entry(&format!("{SKY}world0/sky.properties")).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn unused_images_are_found_after_merging() {
	let pack_dir = temp_dir();
	let p = pack_dir.path();

	write_file(p, &format!("{SKY}world0/sky.properties"), b"source=b.png\n");
	for (name, data) in [("a", &b"same"[..]), ("b", b"same"), ("d", b"other")] {
		write_file(p, &format!("{SKY}world0/{name}.png"), data);
	}

	let optimizer = optimizer(p, true).await;

	// b.png was merged into a.png, which the entry now uses. d.png has no entry
	assert!(optimizer.skip_reason(&format!("{SKY}world0/b.png")).is_some());
	assert_eq!(optimizer.skip_reason(&format!("{SKY}world0/a.png")), None);
	assert_eq!(
		optimizer.skip_reason(&format!("{SKY}world0/d.png")),
		Some("Unused by OptiFine custom sky entries")
	);
}

#[test]
fn file_names_are_only_replaced_as_a_whole() {
	assert_eq!(
		replace_file_name(
			"a=b.png\nb=./b.png\nc = b.png \nd=/x/y/b.png\ne=ab.png\nf=b.png.bak\ng=b.pngx\n# b.png",
			"b.png",
			"kept.png"
		),
		"a=kept.png\nb=./kept.png\nc = kept.png \nd=/x/y/kept.png\ne=ab.png\nf=b.png.bak\ng=b.pngx\n# kept.png"
	);
	assert_eq!(replace_file_name("b.png", "b.png", "k.png"), "k.png");
	assert_eq!(replace_file_name("", "b.png", "k.png"), "");
}

struct SquashedSky {
	output: ZipArchive,
	intentionally_skipped: Vec<String>
}

/// Not async because `PackSquasher::run` starts its own runtime.
fn squash_sky(
	pack_dir: &Path,
	skip_unused_images: bool,
	merge_identical_images: bool,
	allow_optifine: bool
) -> SquashedSky {
	let output_dir = temp_dir();
	let output_path = output_dir.path().join("pack.zip");

	let mut global_options = GlobalOptions {
		skip_unused_optifine_sky_images: skip_unused_images,
		merge_identical_optifine_sky_images: merge_identical_images,
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
				pack_directory: pack_dir.to_path_buf(),
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

	SquashedSky {
		output: ZipArchive::parse(fs::read(output_path).unwrap()).unwrap(),
		intentionally_skipped
	}
}

fn sky_pack(pack_dir: &Path) {
	let image = png(1, 1, &[1, 2, 3, 255], Compression::best());

	write_file(
		pack_dir,
		"pack.mcmeta",
		br#"{"pack": {"pack_format": 1, "description": "Test pack"}}"#
	);
	write_file(pack_dir, &format!("{SKY}world0/sky1.properties"), SKY_ENTRY);
	write_file(
		pack_dir,
		&format!("{SKY}world0/sky2.properties"),
		b"source=./copy.png\nstartFadeIn=8:00\n"
	);
	for name in ["cloud", "copy", "leftover"] {
		write_file(pack_dir, &format!("{SKY}world0/{name}.png"), &image);
	}
	write_file(
		pack_dir,
		&format!("{SKY}world0/other.png"),
		&png(1, 1, &[9, 9, 9, 255], Compression::best())
	);
}

#[test]
fn squashing_leaves_out_sky_images_only_if_asked() {
	let sky_file = |name: &str| format!("{SKY}world0/{name}");

	// (skip unused images, merge identical images, allow OptiFine)
	for (skip_unused, merge, allow_optifine) in [
		(true, false, true),
		(false, true, true),
		(true, true, true),
		(false, false, true),
		(true, true, false)
	] {
		let pack_dir = temp_dir();
		sky_pack(pack_dir.path());

		let SquashedSky {
			output,
			intentionally_skipped
		} = squash_sky(pack_dir.path(), skip_unused, merge, allow_optifine);

		let case = format!("skip unused {skip_unused}, merge {merge}, OptiFine {allow_optifine}");
		let acts = allow_optifine;

		// All three images have the same pixels, so all of them have the same bytes. With
		// merging, cloud.png is kept, and copy.png and leftover.png go away because sky2 is
		// rewritten to use cloud.png. Without it, only leftover.png is unused. other.png is
		// not used by any entry, but is not identical to another image either
		let mut expected_skipped = Vec::new();
		if acts && merge {
			expected_skipped.extend([sky_file("copy.png"), sky_file("leftover.png")]);
		}
		if acts && skip_unused {
			if !merge {
				expected_skipped.push(sky_file("leftover.png"));
			}
			expected_skipped.push(sky_file("other.png"));
		}
		expected_skipped.sort();
		expected_skipped.dedup();

		let mut skipped = intentionally_skipped;
		skipped.sort();
		assert_eq!(skipped, expected_skipped, "{case}");

		if acts {
			for file in ["sky1.properties", "sky2.properties", "cloud.png"] {
				assert!(output.contains(&sky_file(file)), "{file} in {case}");
			}

			let sky2 =
				String::from_utf8(output.read(&sky_file("sky2.properties")).unwrap()).unwrap();
			if merge {
				assert!(sky2.contains("cloud.png") && !sky2.contains("copy.png"), "{sky2}");
			} else {
				assert!(sky2.contains("copy.png"), "{sky2}");
			}
		}
	}
}
