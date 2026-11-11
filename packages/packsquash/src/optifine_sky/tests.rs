use std::fs;

use flate2::Compression;

use super::*;
use crate::config::{GlobalOptions, MinecraftMod, SquashOptions};
use crate::mcchanges::ZipArchive;
use crate::mcchanges::tests::{png, temp_dir, write_file};
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
		merge_identical_images,
		false
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

	assert_eq!(
		optimizer.skip_reason(&format!("{SKY}world0/cloud.png")),
		None
	);
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

	write_file(
		p,
		&format!("{SKY}world0/sky1.properties"),
		b"source=./b.png\nblend=add\n"
	);
	write_file(
		p,
		&format!("{SKY}world0/sky2.properties"),
		b"source = a.png\r\n"
	);
	write_file(
		p,
		&format!("{SKY}world0/sky3.properties"),
		b"source=c.png\n"
	);
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
		assert_eq!(
			optimizer.skip_reason(&format!("{SKY}{kept}")),
			None,
			"{kept}"
		);
	}

	assert_eq!(
		optimizer
			.rewritten_entry(&format!("{SKY}world0/sky1.properties"))
			.as_deref(),
		Some(&b"source=./a.png\nblend=add\n"[..])
	);
	// Entries that do not mention merged images are left alone
	for untouched in [
		"world0/sky2.properties",
		"world0/sky3.properties",
		"world1/sky.properties"
	] {
		assert!(
			optimizer
				.rewritten_entry(&format!("{SKY}{untouched}"))
				.is_none()
		);
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
	write_file(
		p,
		&format!("{SKY}world1/sky.properties"),
		b"source=../world0/b.png\n"
	);

	let optimizer = optimizer(p, true).await;

	assert_eq!(optimizer.skip_reason(&format!("{SKY}world0/b.png")), None);
	assert!(
		optimizer
			.rewritten_entry(&format!("{SKY}world0/sky.properties"))
			.is_none()
	);
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
	assert!(
		optimizer
			.skip_reason(&format!("{SKY}world0/b.png"))
			.is_some()
	);
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
	skip_missing_image_entries: bool,
	allow_optifine: bool
) -> SquashedSky {
	let output_dir = temp_dir();
	let output_path = output_dir.path().join("pack.zip");

	let mut global_options = GlobalOptions {
		skip_unused_optifine_sky_images: skip_unused_images,
		merge_identical_optifine_sky_images: merge_identical_images,
		skip_optifine_sky_entries_with_missing_images: skip_missing_image_entries,
		include_unrecognized_asset_files: false,
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
		} = squash_sky(pack_dir.path(), skip_unused, merge, false, allow_optifine);

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

			let sky2 = String::from_utf8(output.read(&sky_file("sky2.properties")).unwrap()).unwrap();
			if merge {
				assert!(
					sky2.contains("cloud.png") && !sky2.contains("copy.png"),
					"{sky2}"
				);
			} else {
				assert!(sky2.contains("copy.png"), "{sky2}");
			}
		}
	}
}

/// Scans a pack with the option that leaves out the entries whose image is missing.
async fn missing_image_optimizer(pack_dir: &Path) -> SkyImageOptimizer {
	SkyImageOptimizer::scan(
		&OsFilesystem,
		pack_dir,
		IteratorTraversalOptions::default(),
		false,
		false,
		true
	)
	.await
}

const MISSING_IMAGE_REASON: &str = "OptiFine custom sky entry whose image is missing";

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn an_entry_whose_image_is_missing_is_left_out() {
	let pack_dir = temp_dir();
	write_file(
		pack_dir.path(),
		&format!("{SKY}world0/sky1.properties"),
		SKY_ENTRY
	);
	write_file(pack_dir.path(), &format!("{SKY}world0/cloud.png"), b"");
	write_file(
		pack_dir.path(),
		&format!("{SKY}world0/sky2.properties"),
		b"source=./aurora.png\nstartFadeIn=6:00\n"
	);

	let optimizer = missing_image_optimizer(pack_dir.path()).await;

	assert_eq!(
		optimizer.skip_reason(&format!("{SKY}world0/sky1.properties")),
		None
	);
	assert_eq!(
		optimizer.skip_reason(&format!("{SKY}world0/sky2.properties")),
		Some(MISSING_IMAGE_REASON)
	);
	assert!(
		optimizer
			.notices()
			.iter()
			.any(|notice| notice.contains("sky2.properties")
				&& notice.contains("aurora.png")
				&& notice.contains("was left out")),
		"{:?}",
		optimizer.notices()
	);
	assert!(
		optimizer
			.notices()
			.iter()
			.any(|notice| notice.contains("Do not use skip_optifine_sky_entries_with_missing_images")),
		"the warning is there: {:?}",
		optimizer.notices()
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn an_entry_that_a_later_entry_needs_is_kept() {
	let pack_dir = temp_dir();
	let sky = |name: &str| format!("{SKY}world0/{name}");
	write_file(
		pack_dir.path(),
		&sky("sky1.properties"),
		b"source=./aurora.png\n"
	);
	write_file(pack_dir.path(), &sky("sky2.properties"), SKY_ENTRY);
	write_file(pack_dir.path(), &sky("cloud.png"), b"");
	write_file(
		pack_dir.path(),
		&sky("sky3.properties"),
		b"source=./gone.png\n"
	);
	write_file(
		pack_dir.path(),
		&sky("sky4.properties"),
		b"source=./gone2.png\n"
	);

	let optimizer = missing_image_optimizer(pack_dir.path()).await;

	// sky1 is before the working sky2, so it stays. sky3 and sky4 are at the end, so they go
	assert_eq!(optimizer.skip_reason(&sky("sky1.properties")), None);
	assert_eq!(optimizer.skip_reason(&sky("sky2.properties")), None);
	assert_eq!(
		optimizer.skip_reason(&sky("sky3.properties")),
		Some(MISSING_IMAGE_REASON)
	);
	assert_eq!(
		optimizer.skip_reason(&sky("sky4.properties")),
		Some(MISSING_IMAGE_REASON)
	);
	assert!(
		optimizer
			.notices()
			.iter()
			.any(|notice| notice.contains("sky1.properties")
				&& notice.contains("was kept")
				&& notice.contains("stops loading entries at the first missing number"))
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn an_entry_between_missing_ones_is_left_out_with_them() {
	let pack_dir = temp_dir();
	let sky = |name: &str| format!("{SKY}world0/{name}");
	write_file(pack_dir.path(), &sky("sky1.properties"), SKY_ENTRY);
	write_file(pack_dir.path(), &sky("cloud.png"), b"");
	write_file(
		pack_dir.path(),
		&sky("sky2.properties"),
		b"source=./a.png\n"
	);
	write_file(
		pack_dir.path(),
		&sky("sky10.properties"),
		b"source=./b.png\n"
	);

	let optimizer = missing_image_optimizer(pack_dir.path()).await;

	// The numbers are compared as numbers, so sky10 comes after sky2
	assert_eq!(optimizer.skip_reason(&sky("sky1.properties")), None);
	assert_eq!(
		optimizer.skip_reason(&sky("sky2.properties")),
		Some(MISSING_IMAGE_REASON)
	);
	assert_eq!(
		optimizer.skip_reason(&sky("sky10.properties")),
		Some(MISSING_IMAGE_REASON)
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn the_ways_to_name_an_image_follow_optifine() {
	let sky = |name: &str| format!("{SKY}world0/{name}");
	let optifine = "assets/minecraft/optifine/";

	// (the source property, the file that makes the image exist, is the entry left out when the image is not there)
	for (source, image, left_out_without_image) in [
		("./x.png", sky("x.png"), true),
		("~/sky/world0/x.png", sky("x.png"), true),
		(
			"assets/minecraft/optifine/sky/world0/x.png",
			sky("x.png"),
			true
		),
		("minecraft:optifine/sky/world0/x.png", sky("x.png"), true),
		("optifine/sky/world0/x.png", sky("x.png"), true),
		("/~/sky/world0/x.png", sky("x.png"), true),
		("source with spaces ./x.png", sky("x.png"), false),
		// The game or another pack can provide these
		(
			"textures/environment/x.png",
			"assets/minecraft/textures/environment/x.png".to_owned(),
			false
		),
		(
			"minecraft:textures/environment/x.png",
			"assets/minecraft/textures/environment/x.png".to_owned(),
			false
		),
		(
			"other:optifine/sky/x.png",
			"assets/other/optifine/sky/x.png".to_owned(),
			false
		),
		// A path that goes up is not followed
		("../x.png", format!("{optifine}sky/x.png"), false),
		("./../x.png", format!("{optifine}sky/x.png"), false),
		("", sky("x.png"), false),
		("./a\\b.png", sky("x.png"), false)
	] {
		for (has_image, expected_skip) in [(false, left_out_without_image), (true, false)] {
			let pack_dir = temp_dir();
			write_file(
				pack_dir.path(),
				&sky("sky1.properties"),
				format!("source={source}\n").as_bytes()
			);

			if has_image {
				write_file(pack_dir.path(), &image, b"");
			}

			let optimizer = missing_image_optimizer(pack_dir.path()).await;

			assert_eq!(
				optimizer.skip_reason(&sky("sky1.properties")).is_some(),
				expected_skip,
				"source {source:?}, image present: {has_image}"
			);
		}
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn an_entry_without_a_source_uses_the_image_with_its_number() {
	let pack_dir = temp_dir();
	let sky = |name: &str| format!("{SKY}world0/{name}");
	write_file(
		pack_dir.path(),
		&sky("sky1.properties"),
		b"startFadeIn=6:00\n"
	);
	write_file(
		pack_dir.path(),
		&sky("sky2.properties"),
		b"startFadeIn=6:00\n"
	);
	write_file(pack_dir.path(), &sky("sky2.png"), b"");
	// The default of the sun and the moon is not known
	write_file(pack_dir.path(), &sky("sun.properties"), b"blend=add\n");
	write_file(
		pack_dir.path(),
		&sky("moon_phases.properties"),
		b"source=./moon.png\n"
	);

	let optimizer = missing_image_optimizer(pack_dir.path()).await;

	// sky2 has its image, so it stays and sky1 is kept for it
	assert_eq!(optimizer.skip_reason(&sky("sky1.properties")), None);
	assert_eq!(optimizer.skip_reason(&sky("sky2.properties")), None);
	assert_eq!(optimizer.skip_reason(&sky("sun.properties")), None);
	// The ones that are not in the numbered sequence do not depend on the others
	assert_eq!(
		optimizer.skip_reason(&sky("moon_phases.properties")),
		Some(MISSING_IMAGE_REASON)
	);
}

#[test]
fn the_source_of_an_entry_is_read_like_a_properties_file() {
	for (text, expected) in [
		("source=./a.png\n", SourceProperty::Set("./a.png")),
		("source = ./a.png  \n", SourceProperty::Set("./a.png")),
		("source: ./a.png\n", SourceProperty::Set("./a.png")),
		("  source ./a.png\n", SourceProperty::Set("./a.png")),
		(
			"# source=./x.png\n! source=./y.png\nsource=./a.png\n",
			SourceProperty::Set("./a.png")
		),
		(
			"source=./a.png\nsource=./b.png\n",
			SourceProperty::Set("./b.png")
		),
		(
			"sources=./a.png\nsource_x=./a.png\nSource=./a.png\n",
			SourceProperty::Unset
		),
		("startFadeIn=6:00\n", SourceProperty::Unset),
		("", SourceProperty::Unset),
		("source=./a.png\nblend=\\\nadd\n", SourceProperty::Unclear),
		(
			"source=./a.png\r\nblend=add\r\n",
			SourceProperty::Set("./a.png")
		)
	] {
		assert_eq!(source_property(text), expected, "{text:?}");
	}
}

#[test]
fn squashing_leaves_out_entries_with_missing_images_only_if_asked() {
	for (option, allow_optifine, expect_left_out) in [
		(true, true, true),
		(false, true, false),
		(true, false, false)
	] {
		let pack_dir = temp_dir();
		sky_pack(pack_dir.path());
		write_file(
			pack_dir.path(),
			&format!("{SKY}world0/sky3.properties"),
			b"source=./aurora.png\n"
		);

		let SquashedSky {
			output,
			intentionally_skipped
		} = squash_sky(pack_dir.path(), false, false, option, allow_optifine);
		let case = format!("option {option}, OptiFine {allow_optifine}");

		// Without OptiFine support, the entries are not in the result at all
		assert_eq!(
			output.contains(&format!("{SKY}world0/sky3.properties")),
			allow_optifine && !expect_left_out,
			"{case}"
		);
		assert_eq!(
			intentionally_skipped.contains(&format!("{SKY}world0/sky3.properties")),
			expect_left_out,
			"{case}"
		);
		assert_eq!(
			output.contains(&format!("{SKY}world0/sky2.properties")),
			allow_optifine,
			"{case}"
		);
	}
}
