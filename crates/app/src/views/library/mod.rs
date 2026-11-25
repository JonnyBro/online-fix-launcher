mod details;
mod game_log;
mod grid;
mod list;
mod redists;

use rust_i18n::t;
use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, Theme, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use onlinefix_launcher_core::art::{ArtKind, ArtStore};
use onlinefix_launcher_core::fix;
use onlinefix_launcher_core::library::{DEBUG_CHANNELS, Game, env_from_text, env_to_text};
use onlinefix_launcher_core::proton::ProtonBuild;
use onlinefix_launcher_core::tools::WineTool;
use onlinefix_launcher_core::{AppPaths, Command, Engine, PendingStage, RunState};
use serde::{Deserialize, Serialize};

use crate::theme::Mono;
use crate::widgets::{empty_state, format_date, format_play_time, format_size, open_path};

const DETAILS_WIDTH: f32 = 380.;
const CARD_WIDTH: f32 = 160.;
const CARD_GAP: f32 = 16.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sort {
	Name,
	PlayTime,
	LastPlayed,
	Added,
}

impl Sort {
	const ALL: [Sort; 4] = [Sort::Name, Sort::PlayTime, Sort::LastPlayed, Sort::Added];

	fn label(self) -> String {
		match self {
			Sort::Name => t!("lib.sort_name"),
			Sort::PlayTime => t!("lib.sort_play_time"),
			Sort::LastPlayed => t!("lib.sort_last_played"),
			Sort::Added => t!("lib.sort_added"),
		}
		.into_owned()
	}

	/// Names go A to Z; times and dates start with the biggest or newest.
	fn starts_descending(self) -> bool {
		self != Sort::Name
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Layout {
	#[default]
	Grid,
	List,
}

/// How the library is sorted and shown, kept between runs.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryPrefs {
	pub sort: Sort,
	pub descending: bool,
	pub layout: Layout,
	pub show_hidden: bool,
}

impl Default for LibraryPrefs {
	fn default() -> Self {
		Self {
			sort: Sort::LastPlayed,
			descending: true,
			layout: Layout::Grid,
			show_hidden: false,
		}
	}
}

/// Favorites first, then by `sort`; ties go by name.
fn sort_games(games: &mut [&Game], sort: Sort, descending: bool) {
	let by_name = |a: &Game, b: &Game| a.name.to_lowercase().cmp(&b.name.to_lowercase());
	games.sort_by(|a, b| {
		let primary = match sort {
			Sort::Name => by_name(a, b),
			Sort::PlayTime => a.play_seconds.cmp(&b.play_seconds),
			Sort::LastPlayed => a.last_played.cmp(&b.last_played),
			Sort::Added => a.added.cmp(&b.added),
		};
		let primary = if descending {
			primary.reverse()
		} else {
			primary
		};
		b.favorite
			.cmp(&a.favorite)
			.then(primary)
			.then_with(|| by_name(a, b))
	});
}

/// Text fields of the details panel; they edit the draft.
struct Fields {
	name: Entity<InputState>,
	args_before: Entity<InputState>,
	args_after: Entity<InputState>,
	overrides: Entity<InputState>,
	prefix: Entity<InputState>,
	env: Entity<TextareaState>,
	gs_width: Entity<InputState>,
	gs_height: Entity<InputState>,
	gs_game_width: Entity<InputState>,
	gs_game_height: Entity<InputState>,
	gs_refresh: Entity<InputState>,
}

/// Which of the programs the wrapper switches need are installed.
#[derive(Clone, Copy)]
struct Installed {
	mangohud: bool,
	gamemode: bool,
	gamescope: bool,
}

impl Installed {
	fn look() -> Self {
		let has = |name| onlinefix_launcher_core::archive::which(name).is_some();
		Self {
			mangohud: has("mangohud"),
			gamemode: has("gamemoderun"),
			gamescope: has("gamescope"),
		}
	}
}

/// A size field: empty or not a number means "let gamescope pick".
fn parse_size(text: &str) -> u32 {
	text.trim().parse().unwrap_or(0)
}

fn size_text(value: u32) -> String {
	if value == 0 {
		String::new()
	} else {
		value.to_string()
	}
}

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

type MenuAction = Rc<dyn Fn(&mut LibraryView, &mut Window, &mut Context<LibraryView>)>;

/// A game not in the list yet, being unpacked or installed.
struct Unpacking {
	id: String,
	name: String,
	stage: PendingStage,
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
	backups: HashMap<String, Vec<onlinefix_launcher_core::saves::Backup>>,
	prefs: LibraryPrefs,
	installed: Installed,
	redists: HashMap<String, Vec<onlinefix_launcher_core::redist::Redist>>,
	/// Packages the user ticked or unticked; the rest are ticked unless installed.
	redist_pick: HashMap<PathBuf, bool>,
	redist_step: HashMap<String, (usize, usize)>,
	/// The game log shown in place of the games.
	log: Option<game_log::GameLog>,
	debug_channels: Entity<InputState>,
	_log_poll: Task<()>,
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
		let debug_channels = cx.new(|cx| InputState::new(window, cx).placeholder(DEBUG_CHANNELS));
		let log_poll = cx.spawn_in(window, async move |this, cx| {
			loop {
				cx.background_executor()
					.timer(Duration::from_millis(400))
					.await;
				if this
					.update_in(cx, |this, window, cx| this.poll_log(window, cx))
					.is_err()
				{
					break;
				}
			}
		});
		let env = cx.new(|cx| {
			TextareaState::new(window, cx)
				.rows(3)
				.placeholder("DXVK_HUD=fps\nDXVK_FRAME_RATE=60")
		});
		let mut input = |placeholder: String, cx: &mut Context<Self>| {
			cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
		};
		let fields = Fields {
			name: input(String::new(), cx),
			args_before: input(t!("game.args_before_hint").into_owned(), cx),
			args_after: input(t!("game.args_after_hint").into_owned(), cx),
			overrides: input(t!("game.overrides_hint").into_owned(), cx),
			prefix: input(t!("add.prefix_default").into_owned(), cx),
			env,
			gs_width: input(t!("game.auto").into_owned(), cx),
			gs_height: input(t!("game.auto").into_owned(), cx),
			gs_game_width: input(t!("game.auto").into_owned(), cx),
			gs_game_height: input(t!("game.auto").into_owned(), cx),
			gs_refresh: input(t!("game.auto").into_owned(), cx),
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
		let env_changes = cx.subscribe_in(&fields.env, window, |this, input, event, _, cx| {
			if matches!(event, InputEvent::Change)
				&& let Some(draft) = &mut this.draft
			{
				draft.env = env_from_text(&input.read(cx).value());
				cx.notify();
			}
		});
		let subscriptions = vec![
			env_changes,
			cx.subscribe_in(&debug_channels, window, |this, _, event, _, cx| {
				this.channels_event(event, cx)
			}),
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
				&fields.gs_width,
				|g, v| g.gamescope.width = parse_size(&v),
				window,
				cx,
			),
			edit(
				&fields.gs_height,
				|g, v| g.gamescope.height = parse_size(&v),
				window,
				cx,
			),
			edit(
				&fields.gs_game_width,
				|g, v| g.gamescope.game_width = parse_size(&v),
				window,
				cx,
			),
			edit(
				&fields.gs_game_height,
				|g, v| g.gamescope.game_height = parse_size(&v),
				window,
				cx,
			),
			edit(
				&fields.gs_refresh,
				|g, v| g.gamescope.refresh = parse_size(&v),
				window,
				cx,
			),
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
			backups: HashMap::new(),
			prefs: LibraryPrefs::default(),
			installed: Installed::look(),
			redists: HashMap::new(),
			redist_pick: HashMap::new(),
			redist_step: HashMap::new(),
			log: None,
			debug_channels,
			_log_poll: log_poll,
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

	pub fn set_prefs(&mut self, prefs: LibraryPrefs) {
		self.prefs = prefs;
	}

	pub fn prefs(&self) -> LibraryPrefs {
		self.prefs
	}

	/// Picking the current key again flips the order.
	fn set_sort(&mut self, sort: Sort, cx: &mut Context<Self>) {
		if self.prefs.sort == sort {
			self.prefs.descending = !self.prefs.descending;
		} else {
			self.prefs.sort = sort;
			self.prefs.descending = sort.starts_descending();
		}
		cx.notify();
	}

	/// Saved right away, and put into the draft too so saving other edits
	/// later doesn't undo it.
	fn update_saved(&mut self, id: &str, apply: impl Fn(&mut Game), cx: &mut Context<Self>) {
		let Some(mut game) = self.games.iter().find(|g| g.id == id).cloned() else {
			return;
		};
		apply(&mut game);
		if let Some(draft) = self.draft.as_mut().filter(|d| d.id == id) {
			apply(draft);
		}
		self.engine.send(Command::UpdateGame(Box::new(game)));
		cx.notify();
	}

	fn set_favorite(&mut self, id: &str, value: bool, cx: &mut Context<Self>) {
		self.update_saved(id, |g| g.favorite = value, cx);
	}

	fn set_hidden(&mut self, id: &str, value: bool, window: &mut Window, cx: &mut Context<Self>) {
		self.update_saved(id, |g| g.hidden = value, cx);
		if value && !self.prefs.show_hidden && self.selected.as_deref() == Some(id) {
			self.select(None, window, cx);
		}
	}

	/// The right-click menu of a game card or row.
	fn menu(
		&self,
		game: &Game,
		cx: &Context<Self>,
	) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + 'static {
		let view = cx.entity().downgrade();
		let id = game.id.clone();
		let (favorite, hidden) = (game.favorite, game.hidden);
		let running = self.runs.contains_key(&game.id);
		move |menu, _, _| {
			let item = |label: String, f: MenuAction| {
				let view = view.clone();
				PopupMenuItem::new(label).on_click(move |_, window, cx| {
					let f = f.clone();
					let _ = view.update(cx, move |this, cx| f(this, window, cx));
				})
			};
			let id = id.clone();
			let log_id = id.clone();
			let play = {
				let id = id.clone();
				if running {
					item(
						t!("game.stop").into_owned(),
						Rc::new(move |this, _, _| this.engine.send(Command::Stop(id.clone()))),
					)
				} else {
					item(
						t!("game.play").into_owned(),
						Rc::new(move |this, _, _| this.launch(&id)),
					)
				}
			};
			let details = {
				let id = id.clone();
				item(
					t!("lib.menu_details").into_owned(),
					Rc::new(move |this, window, cx| this.select(Some(id.clone()), window, cx)),
				)
			};
			let favorite_item = {
				let id = id.clone();
				item(
					if favorite {
						t!("lib.unfavorite")
					} else {
						t!("lib.favorite")
					}
					.into_owned(),
					Rc::new(move |this, _, cx| this.set_favorite(&id, !favorite, cx)),
				)
			};
			let hide_item = item(
				if hidden {
					t!("lib.unhide")
				} else {
					t!("lib.hide")
				}
				.into_owned(),
				Rc::new(move |this, window, cx| this.set_hidden(&id, !hidden, window, cx)),
			);
			let log = {
				let id = log_id.clone();
				item(
					t!("log.show").into_owned(),
					Rc::new(move |this, window, cx| this.open_log(id.clone(), window, cx)),
				)
			};
			menu.item(play)
				.item(details)
				.item(log)
				.separator()
				.item(favorite_item)
				.item(hide_item)
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
				self.fill_fields(false, window, cx);
			}
			_ => {}
		}
		if let Some(log) = &self.log
			&& !self.games.iter().any(|g| g.id == log.id)
		{
			self.log = None;
		}
		self.fill_channels(false, window, cx);
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

	pub fn set_pending(
		&mut self,
		id: String,
		name: String,
		stage: PendingStage,
		cx: &mut Context<Self>,
	) {
		match self.unpacking.iter_mut().find(|u| u.id == id) {
			Some(u) => u.stage = stage,
			None => self.unpacking.push(Unpacking { id, name, stage }),
		}
		cx.notify();
	}

	pub fn unpack_ended(&mut self, id: &str, cx: &mut Context<Self>) {
		self.unpacking.retain(|u| u.id != id);
		cx.notify();
	}

	/// Games being unpacked or installed, for the status bar.
	pub fn unpacking(&self) -> Vec<(String, PendingStage)> {
		self.unpacking
			.iter()
			.map(|u| (u.name.clone(), u.stage))
			.collect()
	}

	pub fn set_protons(&mut self, protons: Vec<ProtonBuild>, cx: &mut Context<Self>) {
		self.protons = protons;
		cx.notify();
	}

	pub fn set_run(&mut self, id: String, state: RunState, cx: &mut Context<Self>) {
		if state == RunState::Running
			&& let Some(log) = self.log.as_mut().filter(|log| log.id == id)
		{
			log.reset();
		}
		if state == RunState::Stopped {
			self.runs.remove(&id);
		} else {
			self.runs.insert(id, state);
		}
		cx.notify();
	}

	pub fn game_name(&self, id: &str) -> Option<String> {
		self.games
			.iter()
			.find(|g| g.id == id)
			.map(|g| g.name.clone())
	}

	/// Names of games that are up, for the status bar.
	pub fn running(&self) -> Vec<String> {
		self.games
			.iter()
			.filter(|g| self.runs.contains_key(&g.id))
			.map(|g| g.name.clone())
			.collect()
	}

	pub fn set_backups(
		&mut self,
		id: String,
		backups: Vec<onlinefix_launcher_core::saves::Backup>,
		cx: &mut Context<Self>,
	) {
		self.backups.insert(id, backups);
		cx.notify();
	}

	pub fn select(&mut self, id: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(id) = &id {
			self.engine.send(Command::ListBackups(id.clone()));
			self.engine.send(Command::ListRedists(id.clone()));
		}
		if let Some(id) = &id
			&& self.log.as_ref().is_some_and(|log| &log.id != id)
		{
			self.open_log(id.clone(), window, cx);
		}
		self.selected = id;
		self.draft = self.saved().cloned();
		self.fill_fields(true, window, cx);
		cx.notify();
	}

	fn saved(&self) -> Option<&Game> {
		let id = self.selected.as_ref()?;
		self.games.iter().find(|g| &g.id == id)
	}

	fn is_dirty(&self) -> bool {
		self.draft.as_ref() != self.saved()
	}

	/// `force` rewrites the environment text even when it means the same.
	fn fill_fields(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
		let Some(game) = self.draft.clone() else {
			return;
		};
		for (input, text) in [
			(&self.fields.name, game.name),
			(&self.fields.args_before, game.args_before),
			(&self.fields.args_after, game.args_after),
			(&self.fields.overrides, game.extra_overrides),
			(&self.fields.gs_width, size_text(game.gamescope.width)),
			(&self.fields.gs_height, size_text(game.gamescope.height)),
			(
				&self.fields.gs_game_width,
				size_text(game.gamescope.game_width),
			),
			(
				&self.fields.gs_game_height,
				size_text(game.gamescope.game_height),
			),
			(&self.fields.gs_refresh, size_text(game.gamescope.refresh)),
			(
				&self.fields.prefix,
				game.prefix
					.map(|p| p.display().to_string())
					.unwrap_or_default(),
			),
		] {
			input.update(cx, |input, cx| input.set_value(text, window, cx));
		}
		let env = env_to_text(&game.env);
		// Rewriting the text while it's being typed in would eat bad lines.
		if force || env_from_text(&self.fields.env.read(cx).value()) != game.env {
			self.fields
				.env
				.update(cx, |input, cx| input.set_value(env, window, cx));
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
			.filter(|g| self.prefs.show_hidden || !g.hidden)
			.filter(|g| query.is_empty() || g.name.to_lowercase().contains(&query))
			.collect();
		sort_games(&mut games, self.prefs.sort, self.prefs.descending);
		games
	}

	/// What the empty grid or list says: nothing matches the search, or
	/// everything left is hidden.
	fn nothing_shown(&self, cx: &App) -> Div {
		if self.search.read(cx).value().trim().is_empty() {
			empty_state(
				t!("lib.all_hidden").into_owned(),
				t!("lib.all_hidden_hint").into_owned(),
				cx,
			)
		} else {
			empty_state(
				t!("lib.no_match").into_owned(),
				t!("lib.no_match_hint").into_owned(),
				cx,
			)
		}
	}

	fn header(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme();
		let LibraryPrefs {
			sort,
			descending,
			layout,
			show_hidden,
		} = self.prefs;
		let hidden = self.games.iter().filter(|g| g.hidden).count();
		let view = cx.entity().downgrade();
		let layout_button = |id: &'static str, value: Layout, icon: IconName, tip: String| {
			Button::new(id)
				.icon(Icon::new(icon))
				.ghost()
				.small()
				.tooltip(tip)
				.selected(layout == value)
				.on_click(cx.listener(move |this, _, _, cx| {
					this.prefs.layout = value;
					cx.notify();
				}))
		};
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
			// The list sorts by its column headers; the grid has none.
			.when(layout == Layout::Grid && !self.games.is_empty(), |bar| {
				bar.child(
					h_flex()
						.gap_1()
						.child(
							Button::new("sort")
								.label(sort.label())
								.ghost()
								.small()
								.dropdown_caret(true)
								.dropdown_menu(move |mut menu: PopupMenu, _, _| {
									for choice in Sort::ALL {
										let view = view.clone();
										menu = menu.item(
											PopupMenuItem::new(choice.label())
												.checked(choice == sort)
												.on_click(move |_, _, cx| {
													let _ = view.update(cx, |this, cx| {
														this.set_sort(choice, cx)
													});
												}),
										);
									}
									menu
								}),
						)
						.child(
							Button::new("reverse")
								.icon(Icon::new(if descending {
									IconName::SortDescending
								} else {
									IconName::SortAscending
								}))
								.ghost()
								.small()
								.tooltip(t!("lib.reverse"))
								.on_click(cx.listener(|this, _, _, cx| {
									this.prefs.descending = !this.prefs.descending;
									cx.notify();
								})),
						),
				)
			})
			.child(div().flex_1())
			.when(hidden > 0 || show_hidden, |bar| {
				bar.child(
					Button::new("show-hidden")
						.icon(Icon::new(if show_hidden {
							IconName::Eye
						} else {
							IconName::EyeOff
						}))
						.label(hidden.to_string())
						.ghost()
						.small()
						.selected(show_hidden)
						.tooltip(if show_hidden {
							t!("lib.hidden_shown")
						} else {
							t!("lib.hidden_hidden")
						})
						.on_click(cx.listener(|this, _, window, cx| {
							this.prefs.show_hidden = !this.prefs.show_hidden;
							if !this.prefs.show_hidden && this.saved().is_some_and(|g| g.hidden) {
								this.select(None, window, cx);
							}
							cx.notify();
						})),
				)
			})
			.child(
				Input::new(&self.search)
					.prefix(Icon::new(IconName::Search).small())
					.cleanable(true)
					.small()
					.w(px(260.)),
			)
			.child(
				h_flex()
					.gap_1()
					.child(layout_button(
						"layout-grid",
						Layout::Grid,
						IconName::LayoutDashboard,
						t!("lib.view_grid").into(),
					))
					.child(layout_button(
						"layout-list",
						Layout::List,
						IconName::Menu,
						t!("lib.view_list").into(),
					)),
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
		} else if let Some(log) = self.log_view(cx) {
			log
		} else {
			match self.prefs.layout {
				Layout::Grid => self.grid(window, cx),
				Layout::List => self.list(cx),
			}
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

#[cfg(test)]
mod tests {
	// Not `super::*`: gpui's own `test` macro would come with it.
	use super::{Game, Sort, sort_games};

	#[test]
	fn favorites_first_then_sort_then_name() {
		let game = |name: &str, play: u64, added: u64, favorite: bool| Game {
			name: name.into(),
			play_seconds: play,
			added,
			favorite,
			..Game::default()
		};
		let games = [
			game("b", 10, 3, false),
			game("A", 30, 1, false),
			game("c", 10, 2, true),
			game("d", 0, 4, false),
		];
		let order = |sort, descending| {
			let mut list: Vec<&Game> = games.iter().collect();
			sort_games(&mut list, sort, descending);
			list.iter().map(|g| g.name.as_str()).collect::<Vec<_>>()
		};
		assert_eq!(order(Sort::Name, false), ["c", "A", "b", "d"]);
		assert_eq!(order(Sort::Name, true), ["c", "d", "b", "A"]);
		assert_eq!(order(Sort::PlayTime, true), ["c", "A", "b", "d"]);
		assert_eq!(order(Sort::Added, true), ["c", "d", "b", "A"]);
		assert_eq!(order(Sort::LastPlayed, true), ["c", "A", "b", "d"]);
	}
}
