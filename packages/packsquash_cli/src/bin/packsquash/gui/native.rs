//! The window of PackSquash for Windows. It is a normal program window, drawn by FLTK, with
//! no browser. It is black, so it suits OLED screens.

use std::{
	cell::{Cell, RefCell},
	env, fs,
	rc::Rc
};

use fltk::{
	app,
	button::{Button, ToggleButton},
	dialog::{FileDialogType, NativeFileChooser},
	draw,
	enums::{Align, CallbackTrigger, Color, Event, Font, FrameType},
	frame::Frame,
	group::{Scroll, ScrollType},
	input::{FloatInput, Input, IntInput},
	menu::Choice,
	prelude::*,
	text::{TextBuffer, TextDisplay, WrapMode},
	window::Window
};

use super::{
	Shared, lock,
	model::{Kind, Model},
	options::{OPTIONS, TABLES, TABS},
	start
};

// Not `from_hex(0)`: that is the index 0 of FLTK, the foreground color
const BLACK: Color = Color::Black;
const PANEL: Color = Color::from_hex(0x0a0a0a);
const FIELD: Color = Color::from_hex(0x161616);
const HOVER: Color = Color::from_hex(0x222222);
const LINE: Color = Color::from_hex(0x1e1e1e);
const TEXT: Color = Color::from_hex(0xe8e8e8);
const MUTED: Color = Color::from_hex(0x848484);
const ACCENT: Color = Color::from_hex(0x4da3ff);
const WARNING: Color = Color::from_hex(0xf0a030);
const BAD: Color = Color::from_hex(0xe5534b);
const GOOD: Color = Color::from_hex(0x4cc26a);

const SIDE: i32 = 210;
const BAR: i32 = 68;
const HEAD: i32 = 70;
const PAD: i32 = 28;
const CONTROL: i32 = 300;
const SETTINGS_FILE: usize = TABS.len();
const LOG: usize = TABS.len() + 1;

struct Ui {
	model: RefCell<Model>,
	page: Cell<usize>,
	width: Cell<i32>,
	window: Window,
	nav: Vec<Button>,
	title: Frame,
	badge: Frame,
	status: Frame,
	status_id: Cell<u32>,
	scroll: Scroll,
	all: ToggleButton,
	all_label: Frame,
	warnings: Frame,
	toml: TextDisplay,
	log: TextDisplay,
	run: Button,
	stop: Button,
	buttons: Vec<Button>,
	state: Shared,
	log_len: Cell<usize>,
	was_running: Cell<bool>,
	use_cli: Cell<bool>,
	test: Cell<Option<i32>>
}

fn anchor(x: i32, y: i32) {
	let mut frame = Frame::new(x, y, 1, 1, None);

	frame.set_frame(FrameType::NoBox);
}

fn wrap(text: &str, width: i32, size: i32) -> (String, i32) {
	draw::set_font(Font::Helvetica, size);

	let mut lines = Vec::new();
	let mut line = String::new();

	for word in text.split_whitespace() {
		let candidate = if line.is_empty() {
			word.to_string()
		} else {
			format!("{line} {word}")
		};

		if !line.is_empty() && draw::width(&candidate) as i32 > width {
			lines.push(std::mem::take(&mut line));
			line = word.to_string();
		} else {
			line = candidate;
		}
	}

	if !line.is_empty() {
		lines.push(line);
	}

	let count = lines.len().max(1) as i32;

	(lines.join("\n"), count)
}

fn label_bold(x: i32, y: i32, w: i32, h: i32, text: &str, size: i32, color: Color) -> Frame {
	let mut frame = label(x, y, w, h, text, size, color);

	frame.set_label_font(Font::HelveticaBold);

	frame
}

fn label(x: i32, y: i32, w: i32, h: i32, text: &str, size: i32, color: Color) -> Frame {
	let mut frame = Frame::new(x, y, w, h, None);

	frame.set_label(text);
	frame.set_label_size(size);
	frame.set_label_color(color);
	frame.set_label_font(Font::Helvetica);
	frame.set_align(Align::Inside | Align::Left | Align::Top);

	frame
}

fn flat(x: i32, y: i32, w: i32, h: i32, text: &str, color: Color, text_color: Color) -> Button {
	let mut button = Button::new(x, y, w, h, None);

	button.set_label(text);
	button.set_frame(FrameType::RFlatBox);
	button.set_down_frame(FrameType::RFlatBox);
	button.set_color(color);
	button.set_selection_color(ACCENT);
	button.set_label_color(text_color);
	button.set_label_size(13);
	button.clear_visible_focus();
	button.handle(move |button, event| match event {
		Event::Enter if button.active() => {
			button.set_color(if color == ACCENT { TEXT } else { HOVER });
			button.redraw();
			true
		}
		Event::Leave => {
			button.set_color(color);
			button.redraw();
			true
		}
		_ => false
	});

	button
}

fn toggle(x: i32, y: i32, on: bool) -> ToggleButton {
	let mut toggle = ToggleButton::new(x, y, 42, 22, None);

	toggle.set_value(on);
	toggle.clear_visible_focus();
	toggle.draw(|toggle| {
		let (x, y, w, h) = (toggle.x(), toggle.y(), toggle.w(), toggle.h());

		draw::draw_rect_fill(x, y, w, h, BLACK);
		draw::set_draw_color(if toggle.value() {
			ACCENT
		} else {
			Color::from_hex(0x303030)
		});
		draw::draw_pie(x, y, h, h, 0.0, 360.0);
		draw::draw_pie(x + w - h, y, h, h, 0.0, 360.0);
		draw::draw_rectf(x + h / 2, y, w - h, h);
		draw::set_draw_color(if toggle.value() { BLACK } else { TEXT });

		let knob = h - 8;
		let left = if toggle.value() { x + w - h + 4 } else { x + 4 };

		draw::draw_pie(left, y + 4, knob, knob, 0.0, 360.0);
	});

	toggle
}

fn style_input(input: &mut impl InputExt) {
	input.set_color(FIELD);
	input.set_text_color(TEXT);
	input.set_cursor_color(TEXT);
	input.set_selection_color(ACCENT);
	input.set_text_size(13);
	input.set_frame(FrameType::FlatBox);
}

fn style_display(display: &mut TextDisplay, font: Font) {
	display.set_color(PANEL);
	display.set_text_color(TEXT);
	display.set_text_font(font);
	display.set_text_size(13);
	display.set_frame(FrameType::FlatBox);
	display.set_selection_color(ACCENT);
	display.set_scrollbar_size(10);
	display.wrap_mode(WrapMode::AtBounds, 0);
	display.set_buffer(TextBuffer::default());
}

fn style_scrollbar(scroll: &mut Scroll) {
	for mut bar in [scroll.scrollbar(), scroll.hscrollbar()] {
		bar.set_color(BLACK);
		bar.set_selection_color(Color::from_hex(0x3a3a3a));
		bar.set_slider_frame(FrameType::RFlatBox);
		bar.set_frame(FrameType::FlatBox);
	}
}

fn message(ui: &Rc<Ui>, text: &str, color: Color) {
	let id = ui.status_id.get() + 1;
	let mut status = ui.status.clone();

	ui.status_id.set(id);
	status.set_label_color(color);
	status.set_label(text);
	status.redraw();

	let ui = ui.clone();

	app::add_timeout3(5.0, move |_| {
		if ui.status_id.get() == id {
			let mut status = ui.status.clone();

			status.set_label("");
			status.redraw();
		}
	});
}

fn browse(kind: FileDialogType, title: &str) -> Option<String> {
	let mut chooser = NativeFileChooser::new(kind);

	chooser.set_title(title);
	chooser.show();

	let path = chooser.filename();

	(!path.as_os_str().is_empty()).then(|| path.to_string_lossy().into_owned())
}

/// The buttons that choose a path, for the options that are paths.
fn path_buttons(key: &str) -> &'static [(&'static str, FileDialogType)] {
	match key {
		"pack_directory" | "bulk_input_directory" | "mcchanges_path" => &[
			("Folder", FileDialogType::BrowseDir),
			("File", FileDialogType::BrowseFile)
		],
		"bulk_output_directory" => &[("Folder", FileDialogType::BrowseDir)],
		"output_file_path" | "log_file" | "bulk_report_file" => {
			&[("Browse", FileDialogType::BrowseSaveFile)]
		}
		_ => &[]
	}
}

/// Called after a setting changed.
fn changed(ui: &Rc<Ui>) {
	let count = ui.model.borrow().warnings().len();
	let mut badge = ui.badge.clone();

	badge.set_label(&match count {
		0 => String::new(),
		1 => "1 warning, see Settings file".to_string(),
		count => format!("{count} warnings, see Settings file")
	});
	badge.redraw();

	if ui.page.get() == SETTINGS_FILE {
		fill_settings_file(ui);
	}
}

fn fill_settings_file(ui: &Rc<Ui>) {
	let (text, warnings) = {
		let model = ui.model.borrow();

		(model.to_toml(ui.all.value()), model.warnings())
	};
	let mut warning_frame = ui.warnings.clone();
	let (shown, lines) = if warnings.is_empty() {
		("No warnings.".to_string(), 1)
	} else {
		let (text, lines) = wrap(&warnings.join("\n\n"), ui.window.w() - SIDE - 2 * PAD, 12);

		(text, lines + warnings.len() as i32)
	};

	warning_frame.set_label_color(if warnings.is_empty() { MUTED } else { WARNING });
	warning_frame.set_label(&shown);

	let toml = ui.toml.clone();

	if let Some(mut buffer) = toml.buffer() {
		buffer.set_text(&text);
	}

	place(ui, lines);
}

fn place(ui: &Rc<Ui>, warning_lines: i32) {
	let (w, h) = (ui.window.w(), ui.window.h());
	let x = SIDE + PAD;
	let top = HEAD + 40;
	let warning_height = warning_lines.max(1) * 16 + 8;
	let mut warnings = ui.warnings.clone();
	let mut toml = ui.toml.clone();

	warnings.resize(x, top, w - SIDE - 2 * PAD, warning_height);
	toml.resize(
		x,
		top + warning_height + 8,
		w - SIDE - 2 * PAD,
		(h - BAR - top - warning_height - 8 - 12).max(40)
	);
}

fn layout(ui: &Rc<Ui>) {
	let (w, h) = (ui.window.w(), ui.window.h());
	let mut title = ui.title.clone();
	let mut badge = ui.badge.clone();
	let mut status = ui.status.clone();
	let mut scroll = ui.scroll.clone();
	let mut log = ui.log.clone();
	let mut all = ui.all.clone();
	let mut all_label = ui.all_label.clone();

	title.resize(SIDE + PAD, 20, 400, 32);
	badge.resize(w - PAD - 360, 28, 360, 20);
	scroll.resize(SIDE, HEAD, w - SIDE, h - HEAD - BAR);
	log.resize(
		SIDE + PAD,
		HEAD + 4,
		w - SIDE - 2 * PAD,
		h - BAR - HEAD - 16
	);
	all.resize(SIDE + PAD, HEAD + 4, 42, 22);
	all_label.resize(SIDE + PAD + 54, HEAD + 6, 400, 18);

	let y = h - BAR + 22;
	let mut x = SIDE + PAD;

	let mut run = ui.run.clone();
	let mut stop = ui.stop.clone();

	run.resize(x, y, 90, 30);
	x += 98;
	stop.resize(x, y, 76, 30);
	x += 76 + 22;

	for (index, button) in ui.buttons.iter().enumerate() {
		let mut button = button.clone();
		let last = index == ui.buttons.len() - 1;

		if last {
			button.resize(w - PAD - 100, y, 100, 30);
		} else {
			button.resize(x, y, 78, 30);
			x += 86;
		}
	}

	status.resize(x + 8, y + 6, (w - PAD - 100 - x - 24).max(40), 20);

	if ui.page.get() == SETTINGS_FILE {
		fill_settings_file(ui);
	}
}

fn go(ui: &Rc<Ui>, page: usize) {
	ui.page.set(page);

	for (index, button) in ui.nav.iter().enumerate() {
		let mut button = button.clone();
		let selected = index == page;

		button.set_color(if selected { PANEL } else { BLACK });
		button.set_label_color(if selected { ACCENT } else { TEXT });
		button.redraw();
	}

	let mut title = ui.title.clone();
	let mut scroll = ui.scroll.clone();
	let mut all = ui.all.clone();
	let mut all_label = ui.all_label.clone();
	let mut warnings = ui.warnings.clone();
	let mut toml = ui.toml.clone();
	let mut log = ui.log.clone();

	title.set_label(match page {
		SETTINGS_FILE => "Settings file",
		LOG => "Log",
		page => TABS[page]
	});

	for widget in [
		&mut scroll as &mut dyn WidgetExt,
		&mut all,
		&mut all_label,
		&mut warnings,
		&mut toml,
		&mut log
	] {
		widget.hide();
	}

	if page < TABS.len() {
		scroll.show();
		build_rows(ui);
	} else if page == SETTINGS_FILE {
		all.show();
		all_label.show();
		warnings.show();
		toml.show();
		fill_settings_file(ui);
	} else {
		log.show();
	}

	ui.window.clone().redraw();
}

fn build_rows(ui: &Rc<Ui>) {
	let mut scroll = ui.scroll.clone();
	let page = ui.page.get();
	let width = scroll.w() - 12 - 2 * PAD;
	let label_width = width - CONTROL - 24;
	let mut y = scroll.y() + 8;

	ui.width.set(ui.window.w());
	scroll.clear();
	scroll.begin();

	// FLTK measures the scroll position from the first child, so one that sits at the corner
	// keeps it 0
	anchor(scroll.x(), scroll.y());

	let note = match page {
		1 => {
			"Archive input, bulk mode, the debug log, the sky and asset options and a blank mcchanges_version need the fork build that has those features. Bulk mode ignores output_file_path."
		}
		3..=5 => "Each group becomes its own table. Change the pattern to limit it to some files.",
		_ => ""
	};

	if !note.is_empty() {
		let (text, lines) = wrap(note, width, 12);

		label(scroll.x() + PAD, y, width, lines * 16, &text, 12, MUTED);
		y += lines * 16 + 12;
	}

	let mut last_table = None;

	for (index, option) in OPTIONS
		.iter()
		.enumerate()
		.filter(|(_, option)| option.tab == page)
	{
		if option.table != last_table {
			if let Some((table, _, name)) = option
				.table
				.and_then(|table| TABLES.iter().find(|(key, ..)| *key == table))
			{
				y += 6;
				label_bold(scroll.x() + PAD, y, width, 24, name, 17, TEXT);
				y += 32;
				label(
					scroll.x() + PAD,
					y + 6,
					190,
					20,
					"Applies to files matching",
					12,
					MUTED
				);

				let mut glob = Input::new(scroll.x() + PAD + 200, y, width - 200, 28, None);

				style_input(&mut glob);
				glob.set_value(ui.model.borrow().glob(table));
				glob.set_trigger(CallbackTrigger::Changed);

				let (ui, table) = (ui.clone(), *table);

				glob.set_callback(move |input| {
					ui.model.borrow_mut().set_glob(table, &input.value());
					changed(&ui);
				});
				y += 44;
			}

			last_table = option.table;
		}

		y = build_row(ui, &scroll, index, y, width, label_width);
	}

	anchor(scroll.x(), y + 12);
	scroll.end();
	scroll.redraw();
}

fn wire_input<I: InputExt + 'static>(
	input: &mut I,
	ui: &Rc<Ui>,
	index: usize,
	key: &Frame,
	paint: &Rc<dyn Fn(&mut Frame)>
) {
	let range = OPTIONS[index].range;
	let numeric = OPTIONS[index].kind != Kind::Text;
	let (ui, mut key, paint) = (ui.clone(), key.clone(), paint.clone());

	style_input(input);
	input.set_value(ui.model.borrow().text(index));
	input.set_trigger(CallbackTrigger::Changed);
	input.set_callback(move |input| {
		let value = input.value();

		ui.model.borrow_mut().set_text(index, &value);

		let out = numeric
			&& value.trim().parse::<f64>().is_ok_and(|number| {
				range.0.is_some_and(|min| number < min) || range.1.is_some_and(|max| number > max)
			});

		input.set_text_color(if out { BAD } else { TEXT });
		input.redraw();
		(*paint)(&mut key);
		changed(&ui);
	});
}

fn build_row(
	ui: &Rc<Ui>,
	scroll: &Scroll,
	index: usize,
	y: i32,
	width: i32,
	label_width: i32
) -> i32 {
	let option = &OPTIONS[index];
	let x = scroll.x() + PAD;
	let right = x + width - CONTROL;
	let mut hint = option.hint.to_string();

	if !option.default.is_empty() && option.kind != Kind::Bool && option.kind != Kind::Multi {
		if !hint.ends_with('.') {
			hint.push('.');
		}

		hint.push_str(&format!(" Default: {}.", option.default));
	}

	let (hint, hint_lines) = wrap(&hint, label_width, 12);
	let mut key = label_bold(x, y, label_width, 20, option.key, 14, TEXT);

	label(x, y + 22, label_width, hint_lines * 16, &hint, 12, MUTED);

	let paint: Rc<dyn Fn(&mut Frame)> = Rc::new({
		let ui = ui.clone();

		move |key: &mut Frame| {
			key.set_label_color(if ui.model.borrow().changed(index) {
				ACCENT
			} else {
				TEXT
			});
			key.redraw();
		}
	});

	(*paint)(&mut key);

	let mut control_height = 28;

	match option.kind {
		Kind::Bool => {
			let mut switch = toggle(right + CONTROL - 42, y + 2, ui.model.borrow().flag(index));
			let (ui, mut key, paint) = (ui.clone(), key.clone(), paint.clone());

			switch.set_callback(move |switch| {
				ui.model
					.borrow_mut()
					.set_text(index, if switch.value() { "true" } else { "false" });
				(*paint)(&mut key);
				changed(&ui);
			});
			control_height = 26;
		}
		Kind::Choice => {
			let mut choice = Choice::new(right, y, CONTROL, 28, None);

			choice.set_color(FIELD);
			choice.set_text_color(TEXT);
			choice.set_frame(FrameType::FlatBox);
			choice.set_down_frame(FrameType::FlatBox);
			choice.set_selection_color(ACCENT);
			choice.set_label_size(13);
			choice.set_text_size(13);
			choice.clear_visible_focus();

			for item in option.choices {
				choice.add_choice(item);
			}

			let current = ui.model.borrow().text(index).to_string();

			choice.set_value(
				option
					.choices
					.iter()
					.position(|item| *item == current)
					.map_or(0, |position| position as i32)
			);

			let (ui, mut key, paint) = (ui.clone(), key.clone(), paint.clone());

			choice.set_callback(move |choice| {
				if let Some(item) = choice.choice() {
					ui.model.borrow_mut().set_text(index, &item);
				}

				(*paint)(&mut key);
				changed(&ui);
			});
		}
		Kind::Multi => {
			let mut row = y;

			for item in option.choices {
				let mut switch = toggle(
					right,
					row + 1,
					ui.model
						.borrow()
						.items(index)
						.iter()
						.any(|chosen| chosen == *item)
				);

				label(right + 52, row + 2, CONTROL - 52, 20, item, 12, TEXT);

				let (ui, mut key, paint, item) = (ui.clone(), key.clone(), paint.clone(), *item);

				switch.set_callback(move |switch| {
					ui.model.borrow_mut().set_item(index, item, switch.value());
					(*paint)(&mut key);
					changed(&ui);
				});
				row += 28;
			}

			control_height = (row - y).max(28);
		}
		Kind::Int | Kind::Float | Kind::Text => {
			let buttons = path_buttons(option.key);
			let field_width = CONTROL - buttons.len() as i32 * 66;

			match option.kind {
				Kind::Int => {
					let mut input = IntInput::new(right, y, field_width, 28, None);

					wire_input(&mut input, ui, index, &key, &paint);
				}
				Kind::Float => {
					let mut input = FloatInput::new(right, y, field_width, 28, None);

					wire_input(&mut input, ui, index, &key, &paint);
				}
				_ => {
					let mut input = Input::new(right, y, field_width, 28, None);
					let mut x = right + field_width + 6;

					wire_input(&mut input, ui, index, &key, &paint);

					for (name, kind) in buttons {
						let mut button = flat(x, y, 60, 28, name, FIELD, TEXT);
						let (ui, mut input, mut key, paint, kind) =
							(ui.clone(), input.clone(), key.clone(), paint.clone(), *kind);

						button.set_callback(move |_| {
							if let Some(path) = browse(kind, option.key) {
								input.set_value(&path);
								ui.model.borrow_mut().set_text(index, &path);
								input.redraw();
								(*paint)(&mut key);
								changed(&ui);
							}
						});
						x += 66;
					}
				}
			}
		}
	}

	let height = (22 + hint_lines * 16).max(control_height) + 16;
	let mut line = Frame::new(x, y + height - 8, width, 1, None);

	line.set_frame(FrameType::FlatBox);
	line.set_color(LINE);

	y + height + 4
}

fn tick(ui: &Rc<Ui>) {
	let (new_text, running, len) = {
		let guard = lock(&ui.state);
		let old = ui.log_len.get();

		if guard.log.len() < old {
			(
				Some((true, guard.log.clone())),
				guard.running,
				guard.log.len()
			)
		} else if guard.log.len() > old {
			(
				Some((false, guard.log.get(old..).unwrap_or("").to_string())),
				guard.running,
				guard.log.len()
			)
		} else {
			(None, guard.running, old)
		}
	};

	if let Some((reset, text)) = new_text {
		let mut log = ui.log.clone();

		if let Some(mut buffer) = log.buffer() {
			if reset {
				buffer.set_text("");
			}

			buffer.append(&text);

			let end = buffer.length();

			log.set_insert_position(end);
			log.show_insert_position();
		}

		ui.log_len.set(len);
	}

	if running != ui.was_running.get() {
		ui.was_running.set(running);

		let (mut run, mut stop) = (ui.run.clone(), ui.stop.clone());

		if running {
			run.deactivate();
			stop.activate();
		} else {
			run.activate();
			stop.deactivate();

			let code = lock(&ui.state)
				.log
				.rsplit("ended with the code ")
				.next()
				.and_then(|rest| rest.trim().trim_end_matches(']').parse::<i32>().ok());

			match code {
				Some(0) => message(ui, "Done", GOOD),
				Some(code) => message(ui, &format!("PackSquash ended with the code {code}"), BAD),
				None => ()
			}

			if env::var_os("PACKSQUASH_GUI_TEST").is_some() {
				println!(
					"Window test: PackSquash ended with the code {}",
					code.unwrap_or(-1)
				);
				ui.test.set(Some(code.unwrap_or(-1)));
				app::quit();
			}
		}
	}
}

fn run_now(ui: &Rc<Ui>) {
	let text = ui.model.borrow().to_toml(false);

	if text.starts_with("# pack_directory is required") {
		go(ui, 0);
		message(ui, "Choose the pack folder first", WARNING);
		return;
	}

	match start(&ui.state, &text) {
		Ok(()) => {
			go(ui, LOG);
			message(ui, "Running", MUTED);
		}
		Err(error) => message(ui, &error, BAD)
	}
}

fn stop_now(ui: &Rc<Ui>) {
	if let Some(child) = lock(&ui.state).child.as_mut() {
		let _ = child.kill();
	}
}

fn open_file(ui: &Rc<Ui>) {
	let Some(path) = browse(FileDialogType::BrowseFile, "Open an options file") else {
		return;
	};

	load(ui, &path);
}

fn load(ui: &Rc<Ui>, path: &str) {
	let result = fs::read_to_string(path)
		.map_err(|error| error.to_string())
		.and_then(|text| ui.model.borrow_mut().import(&text));

	match result {
		Ok(unknown) if unknown.is_empty() => message(ui, "Opened", GOOD),
		Ok(unknown) => message(
			ui,
			&format!("Opened. Not known here: {}", unknown.join(", ")),
			WARNING
		),
		Err(error) => message(ui, &format!("Could not open it: {error}"), BAD)
	}

	go(
		ui,
		if ui.page.get() < TABS.len() {
			ui.page.get()
		} else {
			0
		}
	);
	changed(ui);
}

fn save_file(ui: &Rc<Ui>) {
	let mut chooser = NativeFileChooser::new(FileDialogType::BrowseSaveFile);

	chooser.set_title("Save the options file");
	chooser.set_preset_file("packsquash.toml");
	chooser.show();

	let path = chooser.filename();

	if path.as_os_str().is_empty() {
		return;
	}

	let text = ui.model.borrow().to_toml(ui.all.value());

	match fs::write(&path, text) {
		Ok(()) => message(ui, "Saved", GOOD),
		Err(error) => message(ui, &format!("Could not save it: {error}"), BAD)
	}
}

/// Opens the window and waits until it is closed. Returns nothing if the person chose to go on
/// in the terminal.
pub fn run(options_file: Option<&str>) -> Option<i32> {
	let app = app::App::default().with_scheme(app::Scheme::Base);

	app::background(0, 0, 0);
	app::background2(22, 22, 22);
	app::foreground(232, 232, 232);
	app::set_selection_color(77, 163, 255);
	app::set_visible_focus(false);
	app::set_font_size(13);

	#[cfg(windows)]
	{
		Font::set_font(Font::Helvetica, "Segoe UI");
		Font::set_font(Font::HelveticaBold, "Segoe UI");
		Font::set_font(Font::Courier, "Consolas");
	}

	let mut window = Window::default()
		.with_size(1120, 740)
		.with_label(&format!("PackSquash {}", env!("CARGO_PKG_VERSION")));

	window.set_color(BLACK);
	window.size_range(1000, 620, 0, 0);

	let mut side = Frame::new(0, 0, SIDE, 740, None);

	side.set_frame(FrameType::FlatBox);
	side.set_color(PANEL);

	let mut brand = label_bold(22, 22, SIDE - 30, 28, "PackSquash", 20, TEXT);

	brand.set_label_color(ACCENT);
	label(22, 52, SIDE - 30, 16, "fork", 12, MUTED);

	let mut nav = Vec::new();

	for (index, name) in TABS.iter().chain(&["Settings file", "Log"]).enumerate() {
		let gap = if index >= TABS.len() { 14 } else { 0 };
		let mut button = Button::new(0, 100 + index as i32 * 42 + gap, SIDE, 38, None);

		button.set_label(&format!("   {name}"));
		button.set_align(Align::Left | Align::Inside);
		button.set_frame(FrameType::FlatBox);
		button.set_down_frame(FrameType::FlatBox);
		button.set_label_size(14);
		button.clear_visible_focus();
		nav.push(button);
	}

	let mut title = label_bold(SIDE + PAD, 20, 400, 32, "", 22, TEXT);
	let mut badge = label(0, 28, 360, 20, "", 12, WARNING);

	badge.set_align(Align::Inside | Align::Right | Align::Top);
	title.set_label_font(Font::HelveticaBold);

	let mut scroll = Scroll::new(SIDE, HEAD, 910, 600, None);

	scroll.set_frame(FrameType::FlatBox);
	scroll.set_color(BLACK);
	scroll.set_type(ScrollType::Vertical);
	style_scrollbar(&mut scroll);
	scroll.end();

	let all = toggle(SIDE + PAD, HEAD + 4, false);
	let all_label = label(
		SIDE + PAD + 54,
		HEAD + 6,
		400,
		18,
		"Write every option, not only the changed ones",
		13,
		TEXT
	);
	let warnings = label(SIDE + PAD, HEAD + 40, 600, 16, "", 12, WARNING);
	let mut toml = TextDisplay::new(SIDE + PAD, HEAD + 80, 800, 400, None);
	let mut log = TextDisplay::new(SIDE + PAD, HEAD + 4, 800, 500, None);

	style_display(&mut toml, Font::Courier);
	style_display(&mut log, Font::Courier);

	let mut bar = Frame::new(SIDE, 740 - BAR, 910, BAR, None);

	bar.set_frame(FrameType::FlatBox);
	bar.set_color(PANEL);

	let run_button = flat(0, 0, 90, 30, "Run", ACCENT, BLACK);
	let mut stop_button = flat(0, 0, 76, 30, "Stop", FIELD, TEXT);

	stop_button.deactivate();

	let buttons = vec![
		flat(0, 0, 78, 30, "Open", FIELD, TEXT),
		flat(0, 0, 78, 30, "Save", FIELD, TEXT),
		flat(0, 0, 78, 30, "Copy", FIELD, TEXT),
		flat(0, 0, 78, 30, "Reset", FIELD, TEXT),
		flat(0, 0, 100, 30, "Use CLI", FIELD, TEXT)
	];
	let mut status = label(0, 0, 200, 20, "", 12, MUTED);

	status.set_align(Align::Inside | Align::Left | Align::Top);

	let mut corner = Frame::new(1119, 739, 1, 1, None);

	corner.hide();
	window.resizable(&corner);
	window.end();

	let ui = Rc::new(Ui {
		model: RefCell::new(Model::default()),
		page: Cell::new(0),
		width: Cell::new(0),
		window: window.clone(),
		nav,
		title,
		badge,
		status,
		status_id: Cell::new(0),
		scroll,
		all,
		all_label,
		warnings,
		toml,
		log,
		run: run_button,
		stop: stop_button,
		buttons,
		state: Shared::default(),
		log_len: Cell::new(0),
		was_running: Cell::new(false),
		use_cli: Cell::new(false),
		test: Cell::new(None)
	});

	for (index, button) in ui.nav.iter().enumerate() {
		let mut button = button.clone();
		let ui = ui.clone();

		button.set_callback(move |_| go(&ui, index));
	}

	{
		let (mut run_button, mut stop_button) = (ui.run.clone(), ui.stop.clone());
		let (run_ui, stop_ui) = (ui.clone(), ui.clone());

		run_button.set_callback(move |_| run_now(&run_ui));
		stop_button.set_callback(move |_| stop_now(&stop_ui));
	}

	{
		let (mut open, mut save, mut copy, mut reset, mut cli) = (
			ui.buttons[0].clone(),
			ui.buttons[1].clone(),
			ui.buttons[2].clone(),
			ui.buttons[3].clone(),
			ui.buttons[4].clone()
		);
		let (open_ui, save_ui, copy_ui, reset_ui, cli_ui) =
			(ui.clone(), ui.clone(), ui.clone(), ui.clone(), ui.clone());

		open.set_callback(move |_| open_file(&open_ui));
		save.set_callback(move |_| save_file(&save_ui));
		copy.set_callback(move |_| {
			app::copy(&copy_ui.model.borrow().to_toml(copy_ui.all.value()));
			message(&copy_ui, "Copied", GOOD);
		});
		reset.set_callback(move |_| {
			*reset_ui.model.borrow_mut() = Model::default();
			go(&reset_ui, reset_ui.page.get().min(TABS.len() - 1));
			changed(&reset_ui);
			message(&reset_ui, "Reset", MUTED);
		});
		cli.set_callback(move |_| {
			if cli_ui.was_running.get() {
				message(&cli_ui, "Stop PackSquash first", WARNING);
			} else {
				cli_ui.use_cli.set(true);
				cli_ui.window.clone().hide();
			}
		});
	}

	{
		let mut all = ui.all.clone();
		let all_ui = ui.clone();

		all.set_callback(move |_| fill_settings_file(&all_ui));
	}

	{
		let mut window = window.clone();
		let close_ui = ui.clone();
		let resize_ui = ui.clone();

		window.set_callback(move |window| {
			if app::event() == Event::Close {
				stop_now(&close_ui);
				window.hide();
			}
		});
		window.resize_callback(move |_, _, _, w, _| {
			layout(&resize_ui);

			if resize_ui.page.get() < TABS.len() && (w - resize_ui.width.get()).abs() > 8 {
				build_rows(&resize_ui);
			}
		});
	}

	if let Some(path) = options_file {
		load(&ui, path);
	}

	layout(&ui);
	go(&ui, 0);
	changed(&ui);

	let mut window = window;

	window.show();
	window.set_cursor(fltk::enums::Cursor::Default);

	let tick_ui = ui.clone();

	app::add_timeout3(0.1, move |handle| {
		tick(&tick_ui);
		app::repeat_timeout3(0.1, handle);
	});

	if env::var_os("PACKSQUASH_GUI_TEST").is_some() {
		let test_ui = ui.clone();

		app::add_timeout3(0.5, move |_| run_now(&test_ui));
	}

	app.run().ok()?;

	if let Some(code) = ui.test.get() {
		return Some(code);
	}

	(!ui.use_cli.get()).then_some(0)
}
