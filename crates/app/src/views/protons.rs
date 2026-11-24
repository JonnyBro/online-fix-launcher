use rust_i18n::t;
use std::path::PathBuf;

use gpui_kit::component::button::Button;
use gpui_kit::component::{Icon, IconName, Sizable};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// Proton builds: the downloaded GE-Proton ones and those Steam has.
pub struct ProtonsView {
	folder: PathBuf,
}

impl ProtonsView {
	pub fn new(folder: PathBuf) -> Self {
		Self { folder }
	}
}

impl Render for ProtonsView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let folder = self.folder.clone();
		crate::widgets::empty_state(
			t!("proton.empty").into_owned(),
			t!("proton.empty_hint").into_owned(),
			cx,
		)
		.child(
			div().pt_2().child(
				Button::new("protons-folder")
					.label(t!("common.open_folder"))
					.icon(Icon::new(IconName::FolderOpen))
					.small()
					.outline()
					.on_click(move |_, _, _| {
						if let Err(e) = std::fs::create_dir_all(&folder) {
							tracing::warn!("{}: {e}", folder.display());
						}
						crate::widgets::open_folder(&folder);
					}),
			),
		)
	}
}
