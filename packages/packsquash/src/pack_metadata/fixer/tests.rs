use super::*;

fn fixed(pack_type: PackType, text: &str) -> Option<(String, Vec<String>)> {
	fix(pack_type, text.as_bytes()).map(|(_, fixes)| {
		(
			String::from_utf8(fixes.fixed_data.unwrap()).unwrap(),
			fixes.fixes.into_iter().map(|fix| fix.summary).collect()
		)
	})
}

const CLIENT: PackType = PackType::ClientResources;

#[test]
fn syntax_the_game_accepts_is_repaired() {
	let (data, fixes) = fixed(
		CLIENT,
		"{'pack': {'pack_format': 15, 'description': 'x',},}"
	)
	.unwrap();

	assert_eq!(data, r#"{"pack":{"pack_format":15,"description":"x"}}"#);
	assert_eq!(fixes.len(), 2);
}

#[test]
fn a_whole_decimal_number_becomes_an_integer() {
	let (data, fixes) = fixed(CLIENT, r#"{"pack":{"pack_format":15.0,"description":"x"}}"#).unwrap();

	assert_eq!(data, r#"{"pack":{"pack_format":15,"description":"x"}}"#);
	assert_eq!(fixes, ["`pack_format` 15.0 is written as 15"]);
}

#[test]
fn a_decimal_number_that_is_not_whole_is_not_fixed() {
	assert!(fixed(CLIENT, r#"{"pack":{"pack_format":15.5,"description":"x"}}"#).is_none());
}

#[test]
fn a_list_with_three_versions_keeps_the_first_and_the_last() {
	let (data, fixes) = fixed(
		CLIENT,
		r#"{"pack":{"pack_format":15,"supported_formats":[15,18,20],"description":"x"}}"#
	)
	.unwrap();

	assert!(data.contains(r#""supported_formats":[15,20]"#), "{data}");
	assert_eq!(
		fixes,
		["`supported_formats` [15,18,20] is written as [15,20]"]
	);
}

#[test]
fn supported_formats_is_removed_for_new_versions() {
	let (data, fixes) = fixed(
		CLIENT,
		r#"{"pack":{"min_format":[65,0],"max_format":[70,0],"supported_formats":[65,70],"description":"x"}}"#
	)
	.unwrap();

	assert!(!data.contains("supported_formats"), "{data}");
	assert_eq!(fixes, ["`supported_formats` is removed"]);
}

#[test]
fn supported_formats_and_pack_format_are_added_for_old_versions() {
	let (data, fixes) = fixed(
		CLIENT,
		r#"{"pack":{"min_format":34,"max_format":46,"description":"x"}}"#
	)
	.unwrap();

	assert!(data.contains(r#""supported_formats":[34,46]"#), "{data}");
	assert!(data.contains(r#""pack_format":34"#), "{data}");
	assert_eq!(fixes.len(), 2);
}

#[test]
fn a_pack_that_does_not_need_a_fix_is_not_changed() {
	assert!(fixed(CLIENT, r#"{"pack":{"pack_format":15,"description":"x"}}"#).is_none());
}

#[test]
fn a_pack_with_a_mistake_that_is_not_known_is_not_changed() {
	assert!(fixed(CLIENT, r#"{"pack":{"description":"x"}}"#).is_none());
	assert!(fixed(CLIENT, "{ this is not json").is_none());
}

#[test]
fn a_line_break_inside_a_string_is_reported_and_the_pack_reads() {
	let text = "{\"pack\":{\"pack_format\":15,\"description\":\"a\nb\"}}";

	assert!(PackMetadata::parse(CLIENT, text.as_bytes()).is_ok());
	assert!(raw_control_characters(text.as_bytes()).is_some());
	assert!(raw_control_characters(br#"{"a":"b"}"#).is_none());
}

#[test]
fn syntax_repair_changes_only_what_is_broken() {
	let fixed = |text: &str| {
		syntax::repair_json(text.as_bytes()).map(|(data, _)| String::from_utf8(data).unwrap())
	};

	assert_eq!(fixed("{\"a\": 1,}").as_deref(), Some("{\"a\": 1}"));
	assert_eq!(fixed("{'a': 'b'}").as_deref(), Some("{\"a\": \"b\"}"));
	assert_eq!(fixed("{\"a\": 1}"), None, "valid JSON has nothing to fix");
	assert_eq!(fixed("{ this is not json"), None);
}
