use std::path::Path;

use super::*;
use crate::mcchanges::tests::{temp_dir, write_file};
use crate::vfs::os_fs::OsFilesystem;

const O: &str = "assets/minecraft/optifine/";

async fn scan(pack_dir: &Path) -> OptifineUnused {
	OptifineUnused::scan(&OsFilesystem, pack_dir, IteratorTraversalOptions::default()).await
}

fn put(pack_dir: &Path, files: &[(&str, &str)]) {
	for (path, content) in files {
		write_file(pack_dir, path, content.as_bytes());
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn connected_textures_are_left_out_when_every_tile_is_missing() {
	let pack_dir = temp_dir();
	put(
		pack_dir.path(),
		&[
			(
				&format!("{O}ctm/gone/a.properties"),
				"method=ctm\ntiles=0-3 7\n"
			),
			(
				&format!("{O}ctm/part/a.properties"),
				"method=ctm\ntiles=0-3\n"
			),
			(&format!("{O}ctm/part/2.png"), ""),
			(
				&format!("{O}ctm/default/a.properties"),
				"tiles=0 <default>\n"
			),
			(
				&format!("{O}ctm/vanilla/a.properties"),
				"tiles=textures/block/stone\n"
			),
			(
				&format!("{O}ctm/named/a.properties"),
				"tiles=./top ~/ctm/named/side\n"
			),
			(&format!("{O}ctm/notiles/a.properties"), "method=ctm\n")
		]
	);

	let unused = scan(pack_dir.path()).await;

	assert_eq!(
		unused.skip_reason(&format!("{O}ctm/gone/a.properties")),
		Some("OptiFine connected textures entry whose tiles are missing")
	);

	for kept in ["part", "default", "vanilla", "notiles"] {
		assert_eq!(
			unused.skip_reason(&format!("{O}ctm/{kept}/a.properties")),
			None,
			"{kept}"
		);
	}

	assert!(
		unused
			.skip_reason(&format!("{O}ctm/named/a.properties"))
			.is_some()
	);
	assert!(unused.notices()[0].contains("2 OptiFine entries"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn custom_items_need_one_file_that_exists() {
	let pack_dir = temp_dir();
	put(
		pack_dir.path(),
		&[
			(
				&format!("{O}cit/a/gone.properties"),
				"type=item\ntexture=missing\n"
			),
			(
				&format!("{O}cit/a/default.properties"),
				"type=item\nitems=stick\n"
			),
			(
				&format!("{O}cit/a/both.properties"),
				"texture=missing\nmodel=./there\n"
			),
			(&format!("{O}cit/a/there.json"), "{}"),
			(
				&format!("{O}cit/a/vanilla.properties"),
				"texture=textures/item/stick\n"
			),
			(
				&format!("{O}cit/a/other.properties"),
				"texture=mod:textures/item/x\n"
			),
			(
				&format!("{O}cit/a/layers.properties"),
				"type=armor\ntexture.diamond_layer_1=./l1\ntexture.diamond_layer_2=./l2\n"
			),
			(&format!("{O}cit/a/l2.png"), ""),
			(&format!("{O}cit/a/own.properties"), "items=stick\n"),
			(&format!("{O}cit/a/own.png"), "")
		]
	);

	let unused = scan(pack_dir.path()).await;
	let skipped = |name: &str| {
		unused
			.skip_reason(&format!("{O}cit/a/{name}.properties"))
			.is_some()
	};

	assert!(skipped("gone"));
	assert!(
		skipped("default"),
		"the default texture has the name of the entry"
	);
	assert!(!skipped("both"));
	assert!(!skipped("vanilla"), "the game can provide it");
	assert!(!skipped("other"), "another pack can provide it");
	assert!(!skipped("layers"));
	assert!(!skipped("own"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn animations_guis_and_colors_use_their_own_keys() {
	let pack_dir = temp_dir();
	put(
		pack_dir.path(),
		&[
			(
				&format!("{O}anim/gone.properties"),
				"from=./nothing.png\nto=textures/block/water_still.png\n"
			),
			(
				&format!("{O}anim/fine.properties"),
				"from=./there.png\nto=textures/block/lava_still.png\n"
			),
			(&format!("{O}anim/there.png"), ""),
			(
				&format!("{O}gui/container/gone.properties"),
				"texture.furnace=./a\ntexture.chest=./b\n"
			),
			(
				&format!("{O}gui/container/fine.properties"),
				"texture.furnace=./a\ntexture.chest=./c\n"
			),
			(&format!("{O}gui/container/c.png"), ""),
			(
				&format!("{O}colormap/custom/gone.properties"),
				"format=grid\nsource=./grass.png\n"
			),
			(
				&format!("{O}colormap/custom/fixed.properties"),
				"format=fixed\ncolor=ff0000\n"
			),
			(
				&format!("{O}colormap/custom/default.properties"),
				"format=vanilla\n"
			)
		]
	);

	let unused = scan(pack_dir.path()).await;

	for (path, skipped) in [
		("anim/gone", true),
		("anim/fine", false),
		("gui/container/gone", true),
		("gui/container/fine", false),
		("colormap/custom/gone", true),
		("colormap/custom/fixed", false),
		("colormap/custom/default", true)
	] {
		assert_eq!(
			unused
				.skip_reason(&format!("{O}{path}.properties"))
				.is_some(),
			skipped,
			"{path}"
		);
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn mcpatcher_entries_and_unclear_entries_are_handled() {
	let pack_dir = temp_dir();
	let m = "assets/minecraft/mcpatcher/";
	put(
		pack_dir.path(),
		&[
			(&format!("{m}ctm/a/gone.properties"), "tiles=~/ctm/a/0\n"),
			(&format!("{m}ctm/a/fine.properties"), "tiles=~/ctm/a/1\n"),
			(&format!("{m}ctm/a/1.png"), ""),
			(&format!("{O}ctm/b/wrapped.properties"), "tiles=0 \\\n 1\n"),
			(&format!("{O}ctm/b/5.png"), ""),
			(
				"assets/minecraft/textures/other/gone.properties",
				"tiles=0\n"
			)
		]
	);

	let unused = scan(pack_dir.path()).await;

	assert!(
		unused
			.skip_reason(&format!("{m}ctm/a/gone.properties"))
			.is_some()
	);
	assert_eq!(
		unused.skip_reason(&format!("{m}ctm/a/fine.properties")),
		None
	);
	assert_eq!(
		unused.skip_reason(&format!("{O}ctm/b/wrapped.properties")),
		None
	);
	assert_eq!(
		unused.skip_reason(&format!("{O}ctm/b/5.png")),
		None,
		"an entry that is not clear keeps its tiles"
	);
	assert_eq!(
		unused.skip_reason("assets/minecraft/textures/other/gone.properties"),
		None
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn tiles_that_no_entry_uses_are_left_out() {
	let pack_dir = temp_dir();
	put(
		pack_dir.path(),
		&[
			(&format!("{O}ctm/a/glass.properties"), "tiles=0-1\n"),
			(&format!("{O}ctm/a/0.png"), ""),
			(&format!("{O}ctm/a/1.png"), ""),
			(&format!("{O}ctm/a/2.png"), ""),
			(&format!("{O}ctm/a/2.png.mcmeta"), "{}"),
			(&format!("{O}ctm/a/top.png"), ""),
			(&format!("{O}ctm/loose/3.png"), ""),
			(
				&format!("{O}ctm/other/stone.properties"),
				"tiles=~/ctm/shared/4\n"
			),
			(&format!("{O}ctm/shared/4.png"), ""),
			(&format!("{O}ctm/shared/5.png"), ""),
			(
				&format!("{O}cit/x/y.properties"),
				"texture=~/ctm/shared/5\n"
			)
		]
	);

	let unused = scan(pack_dir.path()).await;

	for path in ["ctm/a/2.png", "ctm/a/2.png.mcmeta"] {
		assert_eq!(
			unused.skip_reason(&format!("{O}{path}")),
			Some("Unused by OptiFine connected textures entries"),
			"{path}"
		);
	}

	for path in [
		"ctm/a/0.png",
		"ctm/a/1.png",
		"ctm/a/top.png",
		"ctm/loose/3.png",
		"ctm/shared/4.png",
		"ctm/shared/5.png"
	] {
		assert_eq!(unused.skip_reason(&format!("{O}{path}")), None, "{path}");
	}

	assert!(unused.notices()[0].contains("1 connected textures tiles"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn a_pack_without_such_entries_has_nothing_to_leave_out() {
	let pack_dir = temp_dir();
	put(
		pack_dir.path(),
		&[(
			&format!("{O}sky/world0/sky1.properties"),
			"source=./gone.png\n"
		)]
	);

	let unused = scan(pack_dir.path()).await;

	assert!(unused.notices().is_empty());
	assert_eq!(
		unused.skip_reason(&format!("{O}sky/world0/sky1.properties")),
		None
	);
}
