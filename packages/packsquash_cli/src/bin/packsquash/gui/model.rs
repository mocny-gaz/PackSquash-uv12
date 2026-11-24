//! The settings of the window, and the options file that they make. It has no window code, so
//! it has tests.

use super::options::{OPTIONS, TABLES};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
	Bool,
	Int,
	Float,
	Text,
	Choice,
	Multi
}

/// One option of PackSquash that the window shows.
pub struct Opt {
	pub key: &'static str,
	pub tab: usize,
	pub kind: Kind,
	/// The default value as text. A blank default means the option is not set.
	pub default: &'static str,
	pub hint: &'static str,
	/// The values to choose from, for `Choice` and `Multi`.
	pub choices: &'static [&'static str],
	pub range: (Option<f64>, Option<f64>),
	/// The group of files that the option belongs to, if it is a per file option.
	pub table: Option<&'static str>
}

#[derive(Clone, PartialEq, Debug)]
pub enum Value {
	One(String),
	Many(Vec<String>)
}

pub struct Model {
	values: Vec<Value>,
	globs: Vec<String>
}

fn quote(text: &str) -> String {
	if text.contains(['\'', '\n']) {
		let escaped = text
			.replace('\\', "\\\\")
			.replace('"', "\\\"")
			.replace('\n', "\\n");

		format!("\"{escaped}\"")
	} else {
		format!("'{text}'")
	}
}

fn number(text: &str) -> Option<f64> {
	text.trim()
		.parse::<f64>()
		.ok()
		.filter(|number| number.is_finite())
}

pub fn index_of(key: &str) -> Option<usize> {
	OPTIONS.iter().position(|option| option.key == key)
}

impl Default for Model {
	fn default() -> Self {
		Self {
			values: OPTIONS
				.iter()
				.map(|option| {
					if option.kind == Kind::Multi {
						Value::Many(Vec::new())
					} else {
						Value::One(option.default.into())
					}
				})
				.collect(),
			globs: TABLES.iter().map(|(_, glob, _)| (*glob).into()).collect()
		}
	}
}

impl Model {
	pub fn text(&self, option: usize) -> &str {
		match &self.values[option] {
			Value::One(text) => text,
			Value::Many(_) => ""
		}
	}

	pub fn items(&self, option: usize) -> &[String] {
		match &self.values[option] {
			Value::Many(items) => items,
			Value::One(_) => &[]
		}
	}

	pub fn flag(&self, option: usize) -> bool {
		self.text(option) == "true"
	}

	pub fn set_text(&mut self, option: usize, text: &str) {
		self.values[option] = Value::One(text.into());
	}

	pub fn set_item(&mut self, option: usize, item: &str, on: bool) {
		let chosen = OPTIONS[option]
			.choices
			.iter()
			.filter(|choice| {
				if **choice == item {
					on
				} else {
					self.items(option).iter().any(|old| old == **choice)
				}
			})
			.map(|choice| (*choice).to_string())
			.collect();

		self.values[option] = Value::Many(chosen);
	}

	pub fn glob(&self, table: &str) -> &str {
		TABLES
			.iter()
			.position(|(name, ..)| *name == table)
			.map_or("", |index| &self.globs[index])
	}

	pub fn set_glob(&mut self, table: &str, glob: &str) {
		if let Some(index) = TABLES.iter().position(|(name, ..)| *name == table) {
			self.globs[index] = glob.into();
		}
	}

	/// Tells if the value is not the default one, and so it is written.
	pub fn changed(&self, option: usize) -> bool {
		let definition = &OPTIONS[option];

		match &self.values[option] {
			Value::Many(items) => !items.is_empty(),
			Value::One(text) if text.is_empty() => false,
			Value::One(text) => {
				if matches!(definition.kind, Kind::Int | Kind::Float) {
					number(text) != number(definition.default)
				} else {
					text != definition.default
				}
			}
		}
	}

	fn written(&self, option: usize) -> Option<String> {
		let definition = &OPTIONS[option];

		Some(match &self.values[option] {
			Value::Many(items) => {
				format!(
					"[{}]",
					items
						.iter()
						.map(|item| quote(item))
						.collect::<Vec<_>>()
						.join(", ")
				)
			}
			Value::One(text) => match definition.kind {
				Kind::Bool => (text == "true").to_string(),
				Kind::Int | Kind::Float => {
					let number = number(text)?;

					if definition.kind == Kind::Int {
						format!("{}", number.round() as i64)
					} else {
						format!("{number}")
					}
				}
				_ if text.is_empty() => return None,
				_ => quote(text)
			}
		})
	}

	fn bulk(&self) -> bool {
		index_of("bulk_input_directory").is_some_and(|option| !self.text(option).is_empty())
	}

	/// Makes the options file. It has the changed options, or every option with `all`.
	pub fn to_toml(&self, all: bool) -> String {
		let bulk = self.bulk();
		let prefix_hidden =
			index_of("bulk_output_directory").is_some_and(|option| !self.text(option).is_empty());
		let mut top = Vec::new();
		let mut tables = vec![Vec::new(); TABLES.len()];

		for (index, option) in OPTIONS.iter().enumerate() {
			if option.key == "pack_directory" && bulk {
				continue;
			}

			if option.key == "bulk_output_prefix" && prefix_hidden && !self.changed(index) {
				continue;
			}

			if !(all || self.changed(index)) {
				continue;
			}

			let Some(value) = self.written(index) else {
				continue;
			};
			let line = format!("{} = {value}", option.key);

			match option
				.table
				.and_then(|table| TABLES.iter().position(|(name, ..)| *name == table))
			{
				Some(table) => tables[table].push(line),
				None => top.push(line)
			}
		}

		let mut lines = Vec::new();

		if !bulk && index_of("pack_directory").is_some_and(|option| self.text(option).is_empty()) {
			lines.push("# pack_directory is required (or set bulk_input_directory)".to_string());
		}

		lines.extend(top);

		for (table, table_lines) in tables.into_iter().enumerate() {
			if !table_lines.is_empty() {
				lines.push(String::new());
				lines.push(format!("[{}]", quote(&self.globs[table])));
				lines.extend(table_lines);
			}
		}

		lines.join("\n") + "\n"
	}

	/// The mistakes that the settings have, in the words of the old window.
	pub fn warnings(&self) -> Vec<String> {
		let text = |key: &str| index_of(key).map_or("", |option| self.text(option));
		let flag = |key: &str| text(key) == "true";
		let items = |key: &str| index_of(key).map_or(&[][..], |option| self.items(option));
		let mut warnings = Vec::new();
		let (path, version) = (text("mcchanges_path"), text("mcchanges_version"));

		if !version.is_empty() && path.is_empty() {
			warnings.push("mcchanges_version does nothing without mcchanges_path".to_string());
		}

		if !version.is_empty() && path.ends_with(['\\', '/']) {
			warnings.push("mcchanges_path is a folder: it needs mcchanges-java.jar (or mcchanges-bedrock.jar) with exactly that name inside, or a mcchanges.json of its own. Otherwise use the jar file, like C:\\amc\\mcchanges-java.jar".into());
		}

		if !path.is_empty()
			&& version.is_empty()
			&& [".jar", ".zip"]
				.iter()
				.any(|extension| path.to_lowercase().ends_with(extension))
		{
			warnings.push(
				"mcchanges_version is blank: this needs the fork build with version auto-detect"
					.into()
			);
		}

		if !items("work_around_minecraft_quirks").is_empty()
			&& flag("automatic_minecraft_quirks_detection")
		{
			warnings.push("work_around_minecraft_quirks is ignored while automatic_minecraft_quirks_detection is on".into());
		}

		if self.bulk() && !text("pack_directory").is_empty() {
			warnings.push("Bulk mode is on: pack_directory and output_file_path are ignored".into());
		}

		if flag("optifine_sky_fix") {
			warnings.push("optifine_sky_fix: do not use it on a pack that is still being made. If an image is added later, its sky entry is gone".into());

			if !items("allow_mods").iter().any(|item| item == "OptiFine") {
				warnings.push("optifine_sky_fix does nothing unless allow_mods has OptiFine".into());
			}
		}

		if !text("log_file").is_empty() && !flag("log") {
			warnings.push(
				"log_file is set, but log is off. PackSquash stops with an error. Turn log on".into()
			);
		}

		if number(text("zip_compression_iterations")).is_some_and(|passes| passes > 100.0) {
			warnings.push("More than 100 Zopfli passes is very slow".into());
		}

		if flag("bulk_overwrite") && !flag("bulk_backup") {
			warnings.push("Overwrite without backup: the original packs are lost".into());
		}

		let output = text("bulk_output_directory");

		if !output.is_empty() && !self.bulk() {
			warnings.push("bulk_output_directory is set but bulk_input_directory is not: PackSquash stops with an error".into());
		}

		if !output.is_empty() && flag("bulk_overwrite") {
			warnings.push("bulk_overwrite does nothing when bulk_output_directory is set: the originals are never replaced".into());
		}

		if !output.is_empty() && text("bulk_output_prefix") == "squashed_" {
			warnings.push("bulk_output_directory is set: the results have no prefix. To add one, change bulk_output_prefix".into());
		}

		warnings
	}

	/// Replaces the settings with the ones of an options file. Returns the names that are not
	/// options of the window.
	pub fn import(&mut self, text: &str) -> Result<Vec<String>, String> {
		let table: toml::Table = text
			.parse()
			.map_err(|error: toml::de::Error| error.to_string())?;
		let mut unknown = Vec::new();

		*self = Self::default();

		for (section, value) in &table {
			match value {
				toml::Value::Table(inner) => {
					for (key, value) in inner {
						self.import_option(key, value, Some(section), &mut unknown);
					}
				}
				_ => self.import_option(section, value, None, &mut unknown)
			}
		}

		Ok(unknown)
	}

	fn import_option(
		&mut self,
		key: &str,
		value: &toml::Value,
		glob: Option<&str>,
		unknown: &mut Vec<String>
	) {
		let Some(option) = index_of(key) else {
			unknown.push(key.into());
			return;
		};
		let definition = &OPTIONS[option];

		match (definition.kind, value) {
			(Kind::Bool, toml::Value::Boolean(on)) => self.set_text(option, &on.to_string()),
			(Kind::Int, toml::Value::Integer(number)) => self.set_text(option, &number.to_string()),
			(Kind::Int | Kind::Float, toml::Value::Float(number)) => {
				self.set_text(option, &number.to_string());
			}
			(Kind::Float, toml::Value::Integer(number)) => self.set_text(option, &number.to_string()),
			(Kind::Text | Kind::Choice, toml::Value::String(text)) => self.set_text(option, text),
			(Kind::Multi, toml::Value::Array(items)) => {
				for choice in definition.choices {
					let on = items.iter().any(|item| item.as_str() == Some(choice));

					self.set_item(option, choice, on);
				}
			}
			_ => {
				unknown.push(key.into());
				return;
			}
		}

		if let (Some(table), Some(glob)) = (definition.table, glob) {
			self.set_glob(table, glob);
		}
	}
}

/// How many bytes Ctrl+Backspace (`forward` false) or Ctrl+Delete (`forward` true) remove, like Notepad: a word,
/// which is a run of letters and digits or a run of other signs, and the spaces next to it. `before` is the text
/// before the caret and `after` the text after it.
pub fn word_length(before: &str, after: &str, forward: bool) -> usize {
	let class = |c: char| {
		if c.is_whitespace() {
			0
		} else if c.is_alphanumeric() || c == '_' {
			1
		} else {
			2
		}
	};
	let mut length = 0;

	if forward {
		let mut chars = after.chars().peekable();
		let word = chars.peek().map(|c| class(*c)).filter(|class| *class != 0);

		if let Some(word) = word {
			while let Some(c) = chars.next_if(|c| class(*c) == word) {
				length += c.len_utf8();
			}
		}

		while let Some(c) = chars.next_if(|c| class(*c) == 0) {
			length += c.len_utf8();
		}
	} else {
		let mut chars = before.chars().rev().peekable();

		while let Some(c) = chars.next_if(|c| class(*c) == 0) {
			length += c.len_utf8();
		}

		if let Some(word) = chars.peek().map(|c| class(*c)) {
			while let Some(c) = chars.next_if(|c| class(*c) == word) {
				length += c.len_utf8();
			}
		}
	}

	length
}

#[cfg(test)]
mod tests {
	use super::*;

	fn set(model: &mut Model, key: &str, value: &str) {
		model.set_text(index_of(key).unwrap(), value);
	}

	#[test]
	fn the_defaults_write_only_the_required_pack_directory_note() {
		assert_eq!(
			Model::default().to_toml(false),
			"# pack_directory is required (or set bulk_input_directory)\n"
		);
	}

	#[test]
	fn only_changed_options_are_written_and_per_file_options_get_a_table() {
		let mut model = Model::default();

		set(&mut model, "pack_directory", "C:\\my pack");
		set(&mut model, "zip_compression_iterations", "30");
		set(&mut model, "log", "true");
		set(&mut model, "image_data_compression_iterations", "7");
		set(&mut model, "image_data_compression_iterations", "5");
		set(&mut model, "color_quantization_target", "none");
		model.set_item(index_of("allow_mods").unwrap(), "OptiFine", true);
		model.set_glob("png", "textures/**/*.png");

		assert_eq!(
			model.to_toml(false),
			"pack_directory = 'C:\\my pack'\nlog = true\nallow_mods = ['OptiFine']\nzip_compression_iterations = 30\n\n['textures/**/*.png']\ncolor_quantization_target = 'none'\n"
		);
	}

	#[test]
	fn a_blank_number_is_never_written_and_every_option_can_be_written() {
		let all = Model::default().to_toml(true);

		assert!(all.contains("zip_compression_iterations = 20\n"));
		assert!(all.contains("allow_mods = []\n"));
		assert!(!all.contains("threads"));
		assert!(!all.contains("= \n"));
		assert!(all.parse::<toml::Table>().is_ok());
	}

	#[test]
	fn bulk_mode_leaves_out_pack_directory_and_the_default_prefix_with_an_output_folder() {
		let mut model = Model::default();

		set(&mut model, "pack_directory", "x");
		set(&mut model, "bulk_input_directory", "in");
		set(&mut model, "bulk_output_directory", "out");

		assert_eq!(
			model
				.to_toml(true)
				.lines()
				.filter(|line| line.contains("pack_directory") || line.contains("bulk_output_prefix"))
				.count(),
			0
		);
	}

	#[test]
	fn text_with_quotes_and_line_breaks_is_written_so_it_reads_back() {
		let mut model = Model::default();

		set(&mut model, "zip_comment", "it's\na \"test\"");

		let table: toml::Table = model.to_toml(false).parse().unwrap();

		assert_eq!(table["zip_comment"].as_str(), Some("it's\na \"test\""));
	}

	#[test]
	fn what_is_written_is_read_back() {
		let mut model = Model::default();

		set(&mut model, "pack_directory", "C:\\a");
		set(&mut model, "threads", "4");
		set(&mut model, "color_quantization_dithering_level", "0.5");
		set(
			&mut model,
			"shader_source_transformation_strategy",
			"prettify"
		);
		model.set_item(index_of("allow_mods").unwrap(), "OptiFine", true);
		model.set_glob("shader", "**/*.fsh");

		let text = model.to_toml(false);
		let mut other = Model::default();

		assert_eq!(other.import(&text), Ok(Vec::new()));
		assert_eq!(other.to_toml(false), text);
		assert_eq!(other.glob("shader"), "**/*.fsh");
	}

	#[test]
	fn import_tells_about_unknown_names_and_a_broken_file() {
		let mut model = Model::default();

		assert_eq!(
			model.import("nope = 1\nlog = true\n"),
			Ok(vec!["nope".to_string()])
		);
		assert!(model.flag(index_of("log").unwrap()));
		assert!(model.import("log = = =").is_err());
	}

	#[test]
	fn ctrl_backspace_and_ctrl_delete_remove_a_word_like_notepad() {
		assert_eq!(word_length("# abc def ghi", "", false), 3);
		assert_eq!(word_length("# abc def ", "", false), 4);
		assert_eq!(word_length("a = 'b'", "", false), 1);
		assert_eq!(word_length("áé x", "", false), 1);
		assert_eq!(word_length("áé", "", false), 4);
		assert_eq!(word_length("", "abc def", true), 4);
		assert_eq!(word_length("", "  abc", true), 2);
		assert_eq!(word_length("x", "", true), 0);
		assert_eq!(word_length("", "", false), 0);
	}

	#[test]
	fn the_known_mistakes_are_warned_about() {
		let mut model = Model::default();

		assert!(model.warnings().is_empty());

		set(&mut model, "mcchanges_version", "1.21");
		set(&mut model, "bulk_overwrite", "true");
		set(&mut model, "bulk_backup", "false");

		assert_eq!(model.warnings().len(), 2);
	}
}
