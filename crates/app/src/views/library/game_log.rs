use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};

use gpui_kit::component::input::{Editor, EditorState};

use super::*;
use crate::logbuf::highlight::{LevelColors, LogHighlighter, SharedColors};

const MAX_LINES: usize = 5000;
/// With debug channels on Wine can write megabytes a second; past this the
/// panel skips to the newest output instead of catching up line by line.
const MAX_READ: u64 = 4 << 20;

/// The last lines of a file that grows, read a piece at a time.
struct Tail {
	path: PathBuf,
	offset: u64,
	partial: String,
	lines: VecDeque<String>,
	/// There are lines the editor doesn't have yet.
	dirty: bool,
	missing: bool,
}

impl Tail {
	fn new(path: PathBuf) -> Self {
		Self {
			path,
			offset: 0,
			partial: String::new(),
			lines: VecDeque::new(),
			dirty: true,
			missing: false,
		}
	}

	/// Starts over from the top, for a file a new run wrote again.
	fn reset(&mut self) {
		self.offset = 0;
		self.partial.clear();
		self.lines.clear();
		self.dirty = true;
	}

	fn read_new(&mut self) {
		let Ok(mut file) = std::fs::File::open(&self.path) else {
			if !self.missing {
				self.missing = true;
				self.reset();
			}
			return;
		};
		self.missing = false;
		let len = file.metadata().map_or(0, |m| m.len());
		// A new run truncates the file and writes it from the top.
		if len < self.offset {
			self.reset();
		}
		if len == self.offset {
			return;
		}
		let mut from = self.offset;
		if len - from > MAX_READ {
			from = len - MAX_READ;
			self.partial.clear();
		}
		let mut bytes = Vec::new();
		if file.seek(SeekFrom::Start(from)).is_err()
			|| file.take(len - from).read_to_end(&mut bytes).is_err()
		{
			return;
		}
		self.offset = from + bytes.len() as u64;
		self.partial.push_str(&String::from_utf8_lossy(&bytes));
		let Some(end) = self.partial.rfind('\n') else {
			return;
		};
		let done: String = self.partial.drain(..=end).collect();
		self.lines
			.extend(done.lines().map(|l| l.trim_end_matches('\r').to_owned()));
		while self.lines.len() > MAX_LINES {
			self.lines.pop_front();
		}
		self.dirty = true;
	}
}

/// One game's log file, kept up to date while it's open.
pub(super) struct GameLog {
	pub id: String,
	tail: Tail,
	/// Lines the editor has.
	shown: usize,
	editor: Entity<EditorState>,
	colors: SharedColors,
}

impl GameLog {
	pub fn open(id: String, path: PathBuf, window: &mut Window, cx: &mut App) -> Self {
		let colors = SharedColors::default();
		let editor = cx.new(|cx| {
			let mut editor = EditorState::new(window, cx)
				.line_number(false)
				.soft_wrap(false);
			editor.set_readonly(true, cx);
			let colors = colors.clone();
			editor.set_highlighter_factory(
				Rc::new(move |_| Some(Box::new(LogHighlighter::wine(colors.clone())))),
				cx,
			);
			editor.set_highlighter("log", cx);
			editor
		});
		let mut tail = Tail::new(path);
		tail.read_new();
		Self {
			id,
			tail,
			shown: 0,
			editor,
			colors,
		}
	}

	pub fn reset(&mut self) {
		self.tail.reset();
	}

	/// Puts new lines into the editor, keeping it at the bottom if it was there.
	/// Returns whether it did.
	pub fn refresh(&mut self, window: &mut Window, cx: &mut App) -> bool {
		if !self.tail.dirty {
			return false;
		}
		let text = self
			.tail
			.lines
			.iter()
			.map(String::as_str)
			.collect::<Vec<_>>()
			.join("\n");
		let shown = self.shown;
		let updated = self.editor.update(cx, |editor, cx| {
			// Replacing the text drops the selection; wait until the user is done with it.
			if !editor.selected_range().is_empty() {
				return false;
			}
			let at_bottom = editor
				.visible_row_range()
				.is_none_or(|rows| rows.end + 1 >= shown);
			editor.set_value(text, window, cx);
			if at_bottom {
				// Clamped to the last line on the next layout.
				editor.set_scroll_offset(point(px(0.), px(-1e9)), cx);
			}
			true
		});
		if updated {
			self.shown = self.tail.lines.len();
			self.tail.dirty = false;
		}
		updated
	}

	fn line_count(&self) -> usize {
		self.tail.lines.len()
	}
}

impl LibraryView {
	pub(super) fn open_log(&mut self, id: String, window: &mut Window, cx: &mut Context<Self>) {
		// The field is shared between games; what was typed belongs to the old one.
		self.save_channels(cx);
		let path = self.paths.game_log(&id);
		let mut log = GameLog::open(id, path, window, cx);
		log.refresh(window, cx);
		self.log = Some(log);
		self.fill_channels(true, window, cx);
		cx.notify();
	}

	pub(super) fn close_log(&mut self, cx: &mut Context<Self>) {
		self.save_channels(cx);
		self.log = None;
		cx.notify();
	}

	pub(super) fn poll_log(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(log) = &mut self.log else {
			return;
		};
		let missing = log.tail.missing;
		log.tail.read_new();
		if log.refresh(window, cx) || log.tail.missing != missing {
			cx.notify();
		}
	}

	/// Shows the game's own debug channels; without `force`, not while they're
	/// being typed in.
	pub(super) fn fill_channels(
		&mut self,
		force: bool,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let Some(game) = self
			.log
			.as_ref()
			.and_then(|log| self.games.iter().find(|g| g.id == log.id))
		else {
			return;
		};
		let text = game.debug_channels.clone();
		let input = self.debug_channels.clone();
		if force || !input.read(cx).focus_handle(cx).is_focused(window) {
			input.update(cx, |input, cx| input.set_value(text, window, cx));
		}
	}

	fn save_channels(&mut self, cx: &mut Context<Self>) {
		let Some(id) = self.log.as_ref().map(|log| log.id.clone()) else {
			return;
		};
		let text = self.debug_channels.read(cx).value().trim().to_owned();
		if self
			.games
			.iter()
			.any(|g| g.id == id && g.debug_channels != text)
		{
			self.update_saved(&id, move |g| g.debug_channels = text.clone(), cx);
		}
	}

	pub(super) fn channels_event(&mut self, event: &InputEvent, cx: &mut Context<Self>) {
		if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
			self.save_channels(cx);
		}
	}

	pub(super) fn log_view(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
		let theme = cx.theme().clone();
		let log = self.log.as_ref()?;
		let game = self.games.iter().find(|g| g.id == log.id)?.clone();
		log.colors.set(LevelColors {
			error: theme.danger,
			warn: theme.warning,
			debug: theme.muted_foreground,
		});
		let running = self.runs.contains_key(&game.id);
		let path = log.tail.path.clone();
		let id = game.id.clone();
		let toolbar = h_flex()
			.px_5()
			.py_2()
			.gap_3()
			.text_sm()
			.border_b_1()
			.border_color(theme.border)
			.child(
				Button::new("log-back")
					.icon(Icon::new(IconName::ArrowLeft))
					.ghost()
					.small()
					.tooltip(t!("log.back"))
					.on_click(cx.listener(|this, _, _, cx| this.close_log(cx))),
			)
			.child(
				div()
					.min_w_0()
					.truncate()
					.child(t!("log.title", name = game.name).into_owned()),
			)
			.child(
				div()
					.text_color(theme.muted_foreground)
					.whitespace_nowrap()
					.child(t!("logs.count", n = log.line_count()).into_owned()),
			)
			.child(div().flex_1())
			.child(
				Checkbox::new("debug-launch")
					.label(t!("log.debug").into_owned())
					.checked(game.debug)
					.tooltip(t!("log.debug_hint"))
					.on_click(cx.listener(move |this, checked: &bool, _, cx| {
						let checked = *checked;
						this.update_saved(&id, move |g| g.debug = checked, cx);
					})),
			)
			.child(
				Input::new(&self.debug_channels)
					.small()
					.disabled(!game.debug)
					.w(px(300.)),
			)
			.child(
				Button::new("log-open-file")
					.label(t!("log.open_file"))
					.icon(Icon::new(IconName::ExternalLink))
					.ghost()
					.small()
					.disabled(log.tail.missing)
					.on_click(move |_, _, _| open_path(&path)),
			);
		let body = if log.tail.missing {
			empty_state(
				t!("log.empty").into_owned(),
				if running {
					t!("log.empty_running")
				} else {
					t!("log.empty_hint")
				}
				.into_owned(),
				cx,
			)
			.into_any_element()
		} else {
			Editor::new(&log.editor)
				.readonly(true)
				.bordered(false)
				.appearance(false)
				.h_full()
				.into_any_element()
		};
		Some(
			v_flex()
				.size_full()
				.child(toolbar)
				.child(div().flex_1().min_h_0().child(body))
				.into_any_element(),
		)
	}
}

#[cfg(test)]
mod tests {
	use std::io::Write;

	use super::Tail;

	#[test]
	fn follows_growth_partial_lines_and_rewrites() {
		let dir = tempfile::tempdir().unwrap();
		let path = dir.path().join("game.log");
		let mut tail = Tail::new(path.clone());
		tail.read_new();
		assert!(tail.missing && tail.lines.is_empty());

		std::fs::write(&path, "header\n\nfirst\r\nsec").unwrap();
		tail.read_new();
		assert!(!tail.missing);
		assert_eq!(tail.lines, ["header", "", "first"]);

		let mut file = std::fs::OpenOptions::new()
			.append(true)
			.open(&path)
			.unwrap();
		file.write_all(b"ond\nthird\n").unwrap();
		tail.dirty = false;
		tail.read_new();
		assert!(tail.dirty);
		assert_eq!(tail.lines, ["header", "", "first", "second", "third"]);

		tail.dirty = false;
		tail.read_new();
		assert!(!tail.dirty);

		std::fs::write(&path, "new run\n").unwrap();
		tail.read_new();
		assert_eq!(tail.lines, ["new run"]);
	}
}
