use serde::de::IgnoredAny;

fn skip_to_line_end(chars: &[char], mut i: usize) -> usize {
	while i < chars.len() && chars[i] != '\n' {
		i += 1;
	}

	i
}

fn quoted(chars: &[char], start: usize, fixes: &mut Fixes) -> (String, usize) {
	let quote = chars[start];
	let mut out = String::from('"');
	let mut i = start + 1;

	if quote == '\'' {
		fixes.single_quotes = true;
	}

	while i < chars.len() && chars[i] != quote {
		let c = chars[i];

		if c == '\\' && i + 1 < chars.len() {
			if chars[i + 1] == '\'' {
				out.push('\'');
			} else {
				out.push('\\');
				out.push(chars[i + 1]);
			}

			i += 2;

			continue;
		}

		match c {
			'"' => out.push_str("\\\""),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
			c => out.push(c)
		}

		if c.is_control() {
			fixes.control_characters = true;
		}

		i += 1;
	}

	out.push('"');

	(out, i + 1)
}

#[derive(Default)]
struct Fixes {
	byte_order_mark: bool,
	comments: bool,
	single_quotes: bool,
	control_characters: bool,
	unquoted_names: bool,
	trailing_commas: bool
}

impl Fixes {
	fn list(&self) -> Vec<&'static str> {
		[
			(self.byte_order_mark, "removed the byte order mark"),
			(self.comments, "removed comments"),
			(self.single_quotes, "changed single quotes to double quotes"),
			(
				self.control_characters,
				"escaped control characters in strings"
			),
			(self.unquoted_names, "quoted names"),
			(self.trailing_commas, "removed trailing commas")
		]
		.into_iter()
		.filter_map(|(applied, text)| applied.then_some(text))
		.collect()
	}
}

/// Makes the JSON that the game accepts but a strict parser refuses valid. Returns the new bytes and what was
/// fixed, or nothing if the text is not changed or still is not valid JSON.
pub(super) fn repair_json(data: &[u8]) -> Option<(Vec<u8>, Vec<&'static str>)> {
	let text = std::str::from_utf8(data).ok()?;
	let mut fixes = Fixes::default();
	let text = text
		.strip_prefix('\u{feff}')
		.inspect(|_| fixes.byte_order_mark = true)
		.unwrap_or(text);
	let chars: Vec<char> = text.chars().collect();
	let mut out = String::with_capacity(text.len());
	let mut i = 0;

	while i < chars.len() {
		let c = chars[i];
		let next = chars.get(i + 1).copied();

		match c {
			'"' | '\'' => {
				let (string, end) = quoted(&chars, i, &mut fixes);

				out.push_str(&string);
				i = end;
			}
			'/' if next == Some('/') => {
				fixes.comments = true;
				i = skip_to_line_end(&chars, i);
			}
			'#' => {
				fixes.comments = true;
				i = skip_to_line_end(&chars, i);
			}
			'/' if next == Some('*') => {
				fixes.comments = true;
				i += 2;

				while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
					i += 1;
				}

				i += 2;
				out.push(' ');
			}
			c if c.is_ascii_alphabetic() || c == '_' => {
				let start = i;

				while i < chars.len()
					&& (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '_' | '.' | '-'))
				{
					i += 1;
				}

				let word: String = chars[start..i].iter().collect();
				let followed_by_colon = chars[i..].iter().find(|c| !c.is_whitespace()) == Some(&':');

				if followed_by_colon && !matches!(word.as_str(), "true" | "false" | "null") {
					fixes.unquoted_names = true;
					out.push_str(&format!("\"{word}\""));
				} else {
					out.push_str(&word);
				}
			}
			c => {
				out.push(c);
				i += 1;
			}
		}
	}

	let fixed = without_trailing_commas(&out, &mut fixes);

	if fixes.list().is_empty() || serde_json::from_str::<IgnoredAny>(&fixed).is_err() {
		return None;
	}

	Some((fixed.into_bytes(), fixes.list()))
}

fn without_trailing_commas(text: &str, fixes: &mut Fixes) -> String {
	let chars: Vec<char> = text.chars().collect();
	let mut out = String::with_capacity(text.len());
	let mut in_string = false;
	let mut i = 0;

	while i < chars.len() {
		let c = chars[i];

		if in_string {
			out.push(c);

			if c == '\\' && i + 1 < chars.len() {
				out.push(chars[i + 1]);
				i += 1;
			} else if c == '"' {
				in_string = false;
			}
		} else if c == '"' {
			in_string = true;
			out.push(c);
		} else if c == ','
			&& chars[i + 1..]
				.iter()
				.find(|c| !c.is_whitespace())
				.is_some_and(|c| matches!(c, '}' | ']'))
		{
			fixes.trailing_commas = true;
		} else {
			out.push(c);
		}

		i += 1;
	}

	out
}
