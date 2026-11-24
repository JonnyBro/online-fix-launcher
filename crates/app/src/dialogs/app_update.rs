//! Asking about a new release, then downloading it and restarting.

use rust_i18n::t;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{ActiveTheme, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use onlinefix_launcher_core::app_update::{self, AppRelease};
use onlinefix_launcher_core::{Command, Engine, Settings};

enum Stage {
	Ask,
	Downloading(f32),
	Ready,
	Failed(String),
}

pub enum UpdateEvent {
	Restart,
}

pub struct UpdateDialog {
	engine: Rc<Engine>,
	release: AppRelease,
	settings: Settings,
	/// Only a release build in a folder the user can write to updates itself.
	can_install: bool,
	stage: Stage,
}

impl EventEmitter<UpdateEvent> for UpdateDialog {}

impl UpdateDialog {
	pub fn open(
		engine: Rc<Engine>,
		release: AppRelease,
		settings: Settings,
		window: &mut Window,
		cx: &mut App,
	) -> Entity<Self> {
		let can_install = release.download.is_some() && app_update::replaceable_binary().is_some();
		let form = cx.new(|_| Self {
			engine,
			release,
			settings,
			can_install,
			stage: Stage::Ask,
		});
		let dialog_form = form.clone();
		window.open_dialog(cx, move |dialog, _, _| {
			dialog
				.title(t!("upd.title").into_owned())
				.w(px(480.))
				.child(dialog_form.clone())
		});
		form
	}

	pub fn progress(&mut self, p: f32, cx: &mut Context<Self>) {
		self.stage = Stage::Downloading(p);
		cx.notify();
	}

	pub fn installed(&mut self, result: Result<(), String>, cx: &mut Context<Self>) {
		self.stage = match result {
			Ok(()) => Stage::Ready,
			Err(e) => Stage::Failed(e),
		};
		cx.notify();
	}

	fn install(&mut self, cx: &mut Context<Self>) {
		if let Some(download) = self.release.download.clone() {
			self.engine.send(Command::InstallAppUpdate(download));
			self.stage = Stage::Downloading(0.);
			cx.notify();
		}
	}

	fn skip(&self, window: &mut Window, cx: &mut App) {
		let mut settings = self.settings.clone();
		settings.skipped_version = self.release.version.clone();
		self.engine.send(Command::SaveSettings(settings));
		window.close_dialog(cx);
	}
}

impl Render for UpdateDialog {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let note = |text: String| {
			div()
				.text_sm()
				.text_color(theme.muted_foreground)
				.child(text)
		};
		let url = self.release.url.clone();
		let release_page = Button::new("upd-page")
			.label(t!("upd.page"))
			.outline()
			.on_click(move |_, _, cx| cx.open_url(&url));
		let later = Button::new("upd-later")
			.label(t!("upd.later"))
			.ghost()
			.on_click(|_, window, cx| window.close_dialog(cx));
		let (body, buttons): (AnyElement, Vec<AnyElement>) = match &self.stage {
			Stage::Ask => (
				v_flex()
					.gap_2()
					.child(
						div().child(
							t!(
								"upd.ask",
								version = self.release.version,
								current = env!("CARGO_PKG_VERSION")
							)
							.into_owned(),
						),
					)
					.when(!self.can_install, |c| {
						c.child(note(t!("upd.manual").into_owned()))
					})
					.into_any_element(),
				vec![
					Button::new("upd-skip")
						.label(t!("upd.skip"))
						.ghost()
						.on_click(cx.listener(|this, _, window, cx| this.skip(window, cx)))
						.into_any_element(),
					later.into_any_element(),
					if self.can_install {
						Button::new("upd-install")
							.label(t!("upd.install"))
							.primary()
							.on_click(cx.listener(|this, _, _, cx| this.install(cx)))
							.into_any_element()
					} else {
						release_page.into_any_element()
					},
				],
			),
			Stage::Downloading(p) => (
				v_flex()
					.gap_2()
					.child(div().child(t!("upd.downloading").into_owned()))
					.child(Progress::new("upd-progress").value(p * 100.))
					.into_any_element(),
				Vec::new(),
			),
			Stage::Ready => (
				div().child(t!("upd.ready").into_owned()).into_any_element(),
				vec![
					later.into_any_element(),
					Button::new("upd-restart")
						.label(t!("upd.restart"))
						.primary()
						.on_click(cx.listener(|_, _, _, cx| cx.emit(UpdateEvent::Restart)))
						.into_any_element(),
				],
			),
			Stage::Failed(e) => (
				div()
					.text_color(theme.danger)
					.child(e.clone())
					.into_any_element(),
				vec![later.into_any_element(), release_page.into_any_element()],
			),
		};
		v_flex()
			.gap_4()
			.child(body)
			.child(h_flex().justify_end().gap_2().children(buttons))
	}
}
