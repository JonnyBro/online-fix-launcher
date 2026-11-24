mod events;
mod sidebar;
mod status_bar;

use rust_i18n::t;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::menu::{DropdownMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::sidebar::{
	Sidebar, SidebarCollapsible, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu,
	SidebarMenuItem,
};
use gpui_kit::component::{
	ActiveTheme, Icon, IconName, Selectable, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use onlinefix_launcher_core::app_update::AppRelease;
use onlinefix_launcher_core::{AppPaths, Command, Engine, Event, Settings};
use serde::{Deserialize, Serialize};

use crate::dialogs::add_game::AddGameDialog;
use crate::dialogs::art_gallery::ArtGallery;
use crate::icons::AppIcon;
use crate::logbuf::highlight::{LevelColors, LogHighlighter, SharedColors};
use crate::logbuf::{Level as LogLevel, LogBuffer, level_of};
use crate::theme::Mono;
use crate::ui_state::{SavedBounds, UiState};
use crate::views::library::{LibraryEvent, LibraryView};
use crate::views::protons::ProtonsView;
use crate::views::settings::SettingsView;

const LOGS_HEIGHT: f32 = 240.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tab {
	Library,
	Proton,
	Settings,
}

impl Tab {
	fn label(self) -> String {
		match self {
			Tab::Library => t!("tab.library"),
			Tab::Proton => t!("tab.proton"),
			Tab::Settings => t!("tab.settings"),
		}
		.into_owned()
	}

	fn icon(self) -> AppIcon {
		match self {
			Tab::Library => AppIcon::Library,
			Tab::Proton => AppIcon::Proton,
			Tab::Settings => AppIcon::Settings,
		}
	}
}

pub struct Shell {
	engine: Rc<Engine>,
	paths: AppPaths,
	ui_path: PathBuf,
	/// A newer release of the app, found at start.
	app_update: Option<AppRelease>,
	tab: Tab,
	library: Entity<LibraryView>,
	protons: Entity<ProtonsView>,
	settings_view: Entity<SettingsView>,
	logs: LogBuffer,
	logs_open: bool,
	logs_seen: u64,
	log_lines: usize,
	/// The lowest level the logs panel shows.
	log_level: LogLevel,
	/// Error lines already seen in the logs panel.
	errors_seen: u64,
	/// Read-only editor, so the log can be selected, searched and copied.
	log_view: Entity<EditorState>,
	log_colors: SharedColors,
	settings: Settings,
	add_dialog: Option<WeakEntity<AddGameDialog>>,
	gallery: Option<WeakEntity<ArtGallery>>,
	/// Ties engine answers to the dialog that asked.
	next_request: u64,
	_tasks: Vec<Task<()>>,
	_appearance: Subscription,
	_library_events: Subscription,
}

/// What `main` read before the window opened.
pub struct Startup {
	pub paths: AppPaths,
	pub settings: Settings,
	/// Why the settings file couldn't be read, when it couldn't.
	pub load_error: Option<String>,
	pub ui: UiState,
	pub logs: LogBuffer,
}

impl Shell {
	pub fn new(startup: Startup, window: &mut Window, cx: &mut Context<Self>) -> Self {
		let Startup {
			paths,
			settings,
			load_error,
			ui,
			logs,
		} = startup;
		// The engine calls this from its own threads; it only wakes the UI task below.
		let (wake_tx, wake_rx) = smol::channel::unbounded::<()>();
		let engine = Engine::start(
			paths.clone(),
			settings.clone(),
			Arc::new(move || {
				let _ = wake_tx.try_send(());
			}),
		)
		.expect("start engine");
		let engine = Rc::new(engine);

		let events = cx.spawn_in(window, async move |this, cx| {
			while wake_rx.recv().await.is_ok() {
				while wake_rx.try_recv().is_ok() {}
				if this
					.update_in(cx, |this, window, cx| this.drain_events(window, cx))
					.is_err()
				{
					break;
				}
			}
		});
		// Log lines come from any thread; the panel picks them up a few times a second.
		let log_poll = cx.spawn_in(window, async move |this, cx| {
			loop {
				cx.background_executor()
					.timer(Duration::from_millis(400))
					.await;
				if this
					.update_in(cx, |this, window, cx| this.poll_logs(window, cx))
					.is_err()
				{
					break;
				}
			}
		});

		let appearance = cx.observe_window_appearance(window, |this, window, cx| {
			crate::theme::apply(&this.settings, window, cx);
		});
		window.on_window_should_close(cx, {
			let shell = cx.entity().downgrade();
			move |window, cx| {
				let bounds = window.window_bounds();
				let _ = shell.update(cx, |this, cx| this.save_ui(bounds, cx));
				true
			}
		});
		if let Some(error) = load_error {
			cx.defer_in(window, move |this, window, cx| {
				let text = t!("app.settings_load_error", error = error).into_owned();
				this.notify(Notification::error(text), window, cx);
			});
		}

		let library = cx.new(|cx| LibraryView::new(engine.clone(), paths.clone(), window, cx));
		let protons =
			cx.new(|_| ProtonsView::new(engine.clone(), paths.protons_dir(), settings.clone()));
		let settings_view =
			cx.new(|cx| SettingsView::new(engine.clone(), settings.clone(), window, cx));
		let library_events = cx.subscribe_in(
			&library,
			window,
			|this, _, event: &LibraryEvent, window, cx| match event {
				LibraryEvent::Add(path) => this.open_add(path.clone(), window, cx),
				LibraryEvent::PickArt { id, kind } => {
					let request = this.request_id();
					let gallery = ArtGallery::open(
						this.engine.clone(),
						id.clone(),
						*kind,
						request,
						window,
						cx,
					);
					this.gallery = Some(gallery.downgrade());
				}
			},
		);
		let log_colors = SharedColors::default();
		let log_view = cx.new(|cx| {
			let mut editor = EditorState::new(window, cx)
				.line_number(false)
				.soft_wrap(false);
			editor.set_readonly(true, cx);
			let colors = log_colors.clone();
			editor.set_highlighter_factory(
				Rc::new(move |_| Some(Box::new(LogHighlighter::new(colors.clone())))),
				cx,
			);
			editor.set_highlighter("log", cx);
			editor
		});
		if settings.check_app_updates {
			engine.send(Command::CheckAppUpdate);
		}
		if ui.tab == Some(Tab::Proton) {
			protons.update(cx, |p, cx| p.shown(cx));
		}
		Self {
			engine,
			ui_path: paths.ui_state_file(),
			paths,
			app_update: None,
			tab: ui.tab.unwrap_or(Tab::Library),
			library,
			protons,
			settings_view,
			logs,
			logs_open: ui.logs_open,
			logs_seen: 0,
			log_lines: 0,
			log_level: LogLevel::Trace,
			errors_seen: 0,
			log_view,
			log_colors,
			settings,
			add_dialog: None,
			gallery: None,
			next_request: 0,
			_tasks: vec![events, log_poll],
			_appearance: appearance,
			_library_events: library_events,
		}
	}

	fn save_ui(&self, bounds: WindowBounds, cx: &App) {
		UiState {
			window: Some(SavedBounds::from_window(bounds)),
			tab: Some(self.tab),
			logs_open: self.logs_open,
			scale: Some(crate::theme::scale(cx)),
		}
		.save(&self.ui_path);
	}

	/// Every notice opens the logs panel when clicked, where the details are.
	fn notify(&self, note: Notification, window: &mut Window, cx: &mut Context<Self>) {
		let shell = cx.entity().downgrade();
		let note = note.on_click(move |_, window, cx| {
			let _ = shell.update(cx, |this, cx| this.open_logs(window, cx));
		});
		window.push_notification(note, cx);
	}

	fn request_id(&mut self) -> u64 {
		self.next_request += 1;
		self.next_request
	}

	fn open_add(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
		let request = self.request_id();
		let dialog = AddGameDialog::open(
			self.engine.clone(),
			path,
			self.settings.games_dir(),
			request,
			window,
			cx,
		);
		self.add_dialog = Some(dialog.downgrade());
		self.set_tab(Tab::Library, cx);
	}

	/// A dropped folder, archive or exe opens the add dialog; one at a time.
	fn add_dropped(&mut self, paths: &ExternalPaths, window: &mut Window, cx: &mut Context<Self>) {
		if let Some(path) = paths.paths().first() {
			self.open_add(path.clone(), window, cx);
		}
	}

	fn set_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
		self.tab = tab;
		if tab == Tab::Proton {
			self.protons.update(cx, |p, cx| p.shown(cx));
		}
		cx.notify();
	}
}

impl Render for Shell {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let content = match self.tab {
			Tab::Library => self.library.clone().into_any_element(),
			Tab::Proton => self.protons.clone().into_any_element(),
			Tab::Settings => self.settings_view.clone().into_any_element(),
		};
		let drop_tint = cx.theme().primary.opacity(0.08);
		let drop_border = cx.theme().primary;
		v_flex()
			.id("shell")
			.size_full()
			.bg(cx.theme().background)
			.text_color(cx.theme().foreground)
			.drag_over::<ExternalPaths>(move |style, _, _, _| {
				style.bg(drop_tint).border_2().border_color(drop_border)
			})
			.on_drop(cx.listener(|this, paths: &ExternalPaths, window, cx| {
				this.add_dropped(paths, window, cx)
			}))
			.child(
				h_flex()
					.flex_1()
					.min_h_0()
					.child(self.sidebar(cx))
					.child(div().flex_1().min_w_0().h_full().child(content)),
			)
			.when(self.logs_open, |this| this.child(self.logs_panel(cx)))
			.child(self.status_bar(cx))
	}
}
