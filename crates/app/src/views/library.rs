use rust_i18n::t;

use gpui_kit::*;

/// The installed games. Filled in once the library store exists.
pub struct LibraryView;

impl LibraryView {
	pub fn new() -> Self {
		Self
	}
}

impl Render for LibraryView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		crate::widgets::empty_state(
			t!("lib.empty").into_owned(),
			t!("lib.empty_hint").into_owned(),
			cx,
		)
	}
}
