//! Picking a cover or banner among SteamGridDB's, or from a file.

use rust_i18n::t;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use onlinefix_launcher_core::art::ArtKind;
use onlinefix_launcher_core::{ArtOption, ArtSource, Command, Engine};

pub struct ArtGallery {
	engine: Rc<Engine>,
	game: String,
	kind: ArtKind,
	request: u64,
	/// `None` while SteamGridDB is asked.
	options: Option<Result<Vec<ArtOption>, String>>,
}

impl ArtGallery {
	pub fn open(
		engine: Rc<Engine>,
		game: String,
		kind: ArtKind,
		request: u64,
		window: &mut Window,
		cx: &mut App,
	) -> Entity<Self> {
		engine.send(Command::ArtChoices {
			request,
			id: game.clone(),
			kind,
		});
		let form = cx.new(|_| Self {
			engine,
			game,
			kind,
			request,
			options: None,
		});
		let title = match kind {
			ArtKind::Grid => t!("art.pick_cover"),
			_ => t!("art.pick_banner"),
		}
		.into_owned();
		let dialog_form = form.clone();
		window.open_dialog(cx, move |dialog, _, _| {
			dialog
				.title(title.clone())
				.w(px(760.))
				.child(dialog_form.clone())
		});
		form
	}

	pub fn request(&self) -> u64 {
		self.request
	}

	pub fn set_options(&mut self, options: Result<Vec<ArtOption>, String>, cx: &mut Context<Self>) {
		self.options = Some(options);
		cx.notify();
	}

	fn pick_file(&self, window: &mut Window, cx: &mut Context<Self>) {
		let picked = cx.prompt_for_paths(PathPromptOptions {
			files: true,
			directories: false,
			multiple: false,
			prompt: None,
		});
		let engine = self.engine.clone();
		let (id, kind) = (self.game.clone(), self.kind);
		cx.spawn_in(window, async move |_, cx| {
			if let Ok(Ok(Some(paths))) = picked.await
				&& let Some(file) = paths.into_iter().next()
			{
				engine.send(Command::SetArt {
					id,
					kind,
					from: ArtSource::File(file),
				});
				let _ = cx.update(|window, cx| window.close_dialog(cx));
			}
		})
		.detach();
	}
}

impl Render for ArtGallery {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let note = |text: String, color: Hsla| {
			div()
				.py_6()
				.text_color(color)
				.child(text)
				.into_any_element()
		};
		let (width, height) = match self.kind {
			ArtKind::Grid => (110., 165.),
			_ => (220., 71.),
		};
		let body = match &self.options {
			None => note(t!("art.loading").into_owned(), theme.muted_foreground),
			Some(Err(e)) => note(e.clone(), theme.danger),
			Some(Ok(options)) if options.is_empty() => {
				note(t!("art.nothing").into_owned(), theme.muted_foreground)
			}
			Some(Ok(options)) => {
				let tiles = options.iter().enumerate().map(|(ix, option)| {
					let url = option.choice.url.clone();
					let (id, kind) = (self.game.clone(), self.kind);
					let engine = self.engine.clone();
					div()
						.id(("art", ix))
						.p_1()
						.rounded(theme.radius)
						.border_2()
						.border_color(theme.transparent)
						.hover(|d| d.border_color(theme.primary))
						.cursor_pointer()
						.child(
							img(option.thumb.clone())
								.w(px(width))
								.h(px(height))
								.rounded(theme.radius)
								.object_fit(ObjectFit::Cover),
						)
						.on_click(move |_, window, cx| {
							engine.send(Command::SetArt {
								id: id.clone(),
								kind,
								from: ArtSource::Url(url.clone()),
							});
							window.close_dialog(cx);
						})
				});
				div()
					.id("art-grid")
					.max_h(px(520.))
					.overflow_y_scroll()
					.child(h_flex().flex_wrap().gap_2().children(tiles))
					.into_any_element()
			}
		};
		v_flex().gap_3().child(body).child(
			h_flex()
				.gap_2()
				.child(
					Button::new("art-file")
						.label(t!("art.from_file"))
						.small()
						.outline()
						.on_click(cx.listener(|this, _, window, cx| this.pick_file(window, cx))),
				)
				.child(div().flex_1())
				.child(
					Button::new("art-close")
						.label(t!("common.close"))
						.small()
						.ghost()
						.on_click(|_, window, cx| window.close_dialog(cx)),
				),
		)
	}
}
