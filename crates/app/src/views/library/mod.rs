mod details;
mod grid;

use rust_i18n::t;
use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Sizable, Theme, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use onlinefix_launcher_core::art::{ArtKind, ArtStore};
use onlinefix_launcher_core::fix;
use onlinefix_launcher_core::library::Game;
use onlinefix_launcher_core::proton::ProtonBuild;
use onlinefix_launcher_core::{AppPaths, Command, Engine, RunState};

use crate::theme::Mono;
use crate::widgets::{empty_state, format_date, format_play_time, open_path};

const DETAILS_WIDTH: f32 = 380.;
const CARD_WIDTH: f32 = 160.;
const CARD_GAP: f32 = 16.;

/// Text fields of the details panel; they edit the draft.
struct Fields {
	name: Entity<InputState>,
	args_before: Entity<InputState>,
	args_after: Entity<InputState>,
	overrides: Entity<InputState>,
	prefix: Entity<InputState>,
}

/// Pictures found for a game.
#[derive(Default, Clone)]
struct Art {
	grid: Option<PathBuf>,
	hero: Option<PathBuf>,
	icon: Option<PathBuf>,
}

pub enum LibraryEvent {
	/// A folder, archive or exe to add.
	Add(PathBuf),
	PickArt {
		id: String,
		kind: ArtKind,
	},
}

/// An archive being unpacked for a game not in the list yet.
struct Unpacking {
	id: String,
	name: String,
	progress: f32,
}

pub struct LibraryView {
	engine: Rc<Engine>,
	paths: AppPaths,
	games: Vec<Game>,
	protons: Vec<ProtonBuild>,
	runs: HashMap<String, RunState>,
	selected: Option<String>,
	/// The selected game with unsaved edits.
	draft: Option<Game>,
	search: Entity<InputState>,
	fields: Fields,
	store: ArtStore,
	art: HashMap<String, Art>,
	unpacking: Vec<Unpacking>,
	_subscriptions: Vec<Subscription>,
}

impl LibraryView {
	pub fn new(
		engine: Rc<Engine>,
		paths: AppPaths,
		window: &mut Window,
		cx: &mut Context<Self>,
	) -> Self {
		let search = cx.new(|cx| InputState::new(window, cx).placeholder(t!("lib.search")));
		let mut input = |placeholder: String, cx: &mut Context<Self>| {
			cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
		};
		let fields = Fields {
			name: input(String::new(), cx),
			args_before: input(t!("game.args_before_hint").into_owned(), cx),
			args_after: input(t!("game.args_after_hint").into_owned(), cx),
			overrides: input(t!("game.overrides_hint").into_owned(), cx),
			prefix: input(t!("add.prefix_default").into_owned(), cx),
		};
		let edit = |input: &Entity<InputState>,
		            apply: fn(&mut Game, String),
		            window: &mut Window,
		            cx: &mut Context<Self>| {
			cx.subscribe_in(input, window, move |this, input, event, _, cx| {
				if matches!(event, InputEvent::Change)
					&& let Some(draft) = &mut this.draft
				{
					apply(draft, input.read(cx).value().to_string());
					cx.notify();
				}
			})
		};
		let subscriptions = vec![
			cx.subscribe_in(&search, window, |_, _, event, _, cx| {
				if matches!(event, InputEvent::Change) {
					cx.notify();
				}
			}),
			edit(&fields.name, |g, v| g.name = v, window, cx),
			edit(&fields.args_before, |g, v| g.args_before = v, window, cx),
			edit(&fields.args_after, |g, v| g.args_after = v, window, cx),
			edit(&fields.overrides, |g, v| g.extra_overrides = v, window, cx),
			edit(
				&fields.prefix,
				|g, v| {
					let v = v.trim();
					g.prefix = (!v.is_empty()).then(|| PathBuf::from(v));
				},
				window,
				cx,
			),
		];
		let store = ArtStore::new(paths.art_dir());
		Self {
			engine,
			paths,
			store,
			art: HashMap::new(),
			unpacking: Vec::new(),
			games: Vec::new(),
			protons: Vec::new(),
			runs: HashMap::new(),
			selected: None,
			draft: None,
			search,
			fields,
			_subscriptions: subscriptions,
		}
	}

	pub fn set_games(&mut self, games: Vec<Game>, window: &mut Window, cx: &mut Context<Self>) {
		let before = self.saved().cloned();
		self.games = games;
		self.art = self
			.games
			.iter()
			.map(|g| (g.id.clone(), self.read_art(&g.id)))
			.collect();
		match (&self.selected, self.saved().cloned()) {
			(Some(_), None) => self.select(None, window, cx),
			// Saved elsewhere (play time after a run); keep edits in progress.
			(Some(_), Some(saved)) if self.draft.as_ref() == before.as_ref() => {
				self.draft = Some(saved);
				self.fill_fields(window, cx);
			}
			_ => {}
		}
		cx.notify();
	}

	fn read_art(&self, id: &str) -> Art {
		Art {
			grid: self.store.find(id, ArtKind::Grid),
			hero: self.store.find(id, ArtKind::Hero),
			icon: self.store.find(id, ArtKind::Icon),
		}
	}

	pub fn art_changed(&mut self, id: String, cx: &mut Context<Self>) {
		let art = self.read_art(&id);
		self.art.insert(id, art);
		cx.notify();
	}

	pub fn set_unpacking(
		&mut self,
		id: String,
		name: String,
		progress: f32,
		cx: &mut Context<Self>,
	) {
		match self.unpacking.iter_mut().find(|u| u.id == id) {
			Some(u) => u.progress = progress,
			None => self.unpacking.push(Unpacking { id, name, progress }),
		}
		cx.notify();
	}

	pub fn unpack_ended(&mut self, id: &str, cx: &mut Context<Self>) {
		self.unpacking.retain(|u| u.id != id);
		cx.notify();
	}

	/// Names and progress of archives being unpacked, for the status bar.
	pub fn unpacking(&self) -> Vec<(String, f32)> {
		self.unpacking
			.iter()
			.map(|u| (u.name.clone(), u.progress))
			.collect()
	}

	pub fn set_protons(&mut self, protons: Vec<ProtonBuild>, cx: &mut Context<Self>) {
		self.protons = protons;
		cx.notify();
	}

	pub fn set_run(&mut self, id: String, state: RunState, cx: &mut Context<Self>) {
		if state == RunState::Stopped {
			self.runs.remove(&id);
		} else {
			self.runs.insert(id, state);
		}
		cx.notify();
	}

	/// Names of games that are up, for the status bar.
	pub fn running(&self) -> Vec<String> {
		self.games
			.iter()
			.filter(|g| self.runs.contains_key(&g.id))
			.map(|g| g.name.clone())
			.collect()
	}

	pub fn select(&mut self, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
		self.selected = id;
		self.draft = self.saved().cloned();
		self.fill_fields(window, cx);
		cx.notify();
	}

	fn saved(&self) -> Option<&Game> {
		let id = self.selected.as_ref()?;
		self.games.iter().find(|g| &g.id == id)
	}

	fn is_dirty(&self) -> bool {
		self.draft.as_ref() != self.saved()
	}

	fn fill_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Some(game) = self.draft.clone() else {
			return;
		};
		for (input, text) in [
			(&self.fields.name, game.name),
			(&self.fields.args_before, game.args_before),
			(&self.fields.args_after, game.args_after),
			(&self.fields.overrides, game.extra_overrides),
			(
				&self.fields.prefix,
				game.prefix
					.map(|p| p.display().to_string())
					.unwrap_or_default(),
			),
		] {
			input.update(cx, |input, cx| input.set_value(text, window, cx));
		}
	}

	fn pick_source(&mut self, folders: bool, window: &mut Window, cx: &mut Context<Self>) {
		let picked = cx.prompt_for_paths(PathPromptOptions {
			files: !folders,
			directories: folders,
			multiple: false,
			prompt: Some(t!("lib.pick_source").into()),
		});
		cx.spawn_in(window, async move |this, cx| {
			if let Ok(Ok(Some(paths))) = picked.await
				&& let Some(path) = paths.into_iter().next()
			{
				let _ = this.update(cx, |_, cx| cx.emit(LibraryEvent::Add(path)));
			}
		})
		.detach();
	}

	fn add_button(
		&self,
		id: &'static str,
		small: bool,
		cx: &mut Context<Self>,
	) -> impl IntoElement {
		let view = cx.entity().downgrade();
		Button::new(id)
			.label(t!("lib.add"))
			.icon(Icon::new(IconName::Plus))
			.primary()
			.when(small, |b| b.small())
			.dropdown_caret(true)
			.dropdown_menu(move |menu, _, _| {
				let item = |label: String, folders: bool| {
					let view = view.clone();
					PopupMenuItem::new(label).on_click(move |_, window, cx| {
						let _ = view.update(cx, |this, cx| this.pick_source(folders, window, cx));
					})
				};
				menu.item(item(t!("lib.add_folder").into_owned(), true))
					.item(item(t!("lib.add_file").into_owned(), false))
			})
	}

	fn launch(&self, id: &str) {
		self.engine.send(Command::Launch(id.to_owned()));
	}

	fn visible(&self, cx: &App) -> Vec<&Game> {
		let query = self.search.read(cx).value().trim().to_lowercase();
		let mut games: Vec<&Game> = self
			.games
			.iter()
			.filter(|g| query.is_empty() || g.name.to_lowercase().contains(&query))
			.collect();
		// Recently played first, then the rest by name.
		games.sort_by(|a, b| {
			b.last_played
				.cmp(&a.last_played)
				.then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
		});
		games
	}

	fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme();
		h_flex()
			.px_5()
			.pt_4()
			.pb_2()
			.gap_3()
			.child(div().text_xl().child(t!("tab.library").into_owned()))
			.child(
				div()
					.text_color(theme.muted_foreground)
					.child(self.games.len().to_string()),
			)
			.child(div().flex_1())
			.child(
				Input::new(&self.search)
					.prefix(Icon::new(IconName::Search).small())
					.cleanable(true)
					.small()
					.w(px(260.)),
			)
			.child(self.add_button("add-game", true, cx))
	}
}

/// The game's picture, or its icon, or its first letter, on a tint picked
/// from the id so games keep their color through a rename.
fn cover(
	game: &Game,
	picture: Option<&PathBuf>,
	icon: Option<&PathBuf>,
	size: (f32, f32),
	theme: &Theme,
) -> AnyElement {
	if let Some(picture) = picture {
		return img(picture.clone())
			.w(px(size.0))
			.h(px(size.1))
			.flex_shrink_0()
			.rounded(theme.radius)
			.object_fit(ObjectFit::Cover)
			.into_any_element();
	}
	let hue = game
		.id
		.bytes()
		.fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(u32::from(b)))
		% 360;
	let tint = hsla(hue as f32 / 360., 0.45, 0.45, 1.);
	let face = match icon {
		Some(icon) => img(icon.clone())
			.size(px(size.0.min(size.1) * 0.45))
			.into_any_element(),
		None => div()
			.text_3xl()
			.text_color(theme.foreground)
			.child(
				game.name
					.chars()
					.find(|c| c.is_alphanumeric())
					.map(|c| c.to_uppercase().to_string())
					.unwrap_or_default(),
			)
			.into_any_element(),
	};
	div()
		.w(px(size.0))
		.h(px(size.1))
		.flex_shrink_0()
		.rounded(theme.radius)
		.bg(tint.opacity(0.35))
		.border_1()
		.border_color(tint.opacity(0.6))
		.flex()
		.items_center()
		.justify_center()
		.child(face)
		.into_any_element()
}

impl EventEmitter<LibraryEvent> for LibraryView {}

impl Render for LibraryView {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let body = if self.games.is_empty() && self.unpacking.is_empty() {
			empty_state(
				t!("lib.empty").into_owned(),
				t!("lib.empty_hint").into_owned(),
				cx,
			)
			.child(div().pt_2().child(self.add_button("add-first", false, cx)))
			.into_any_element()
		} else {
			self.grid(window, cx)
		};
		let details = self.details(window, cx);
		v_flex().size_full().child(self.header(cx)).child(
			h_flex()
				.flex_1()
				.min_h_0()
				.border_t_1()
				.border_color(cx.theme().border)
				.child(div().flex_1().min_w_0().h_full().child(body))
				.children(details),
		)
	}
}
