//! Pieces every view builds with.

use std::path::Path;

use gpui_kit::component::{ActiveTheme, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// A centered message for a view with nothing to show yet.
pub fn empty_state(title: String, hint: String, cx: &App) -> Div {
	let theme = cx.theme();
	v_flex()
		.size_full()
		.items_center()
		.justify_center()
		.gap_2()
		.p_8()
		.child(div().text_lg().child(title))
		.child(
			div()
				.max_w(px(460.))
				.text_center()
				.text_color(theme.muted_foreground)
				.child(hint),
		)
}

pub fn open_folder(path: &Path) {
	if let Err(e) = std::process::Command::new("xdg-open").arg(path).spawn() {
		tracing::warn!("could not open {}: {e}", path.display());
	}
}
