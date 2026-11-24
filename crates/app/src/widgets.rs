//! Pieces every view builds with.

use rust_i18n::t;
use std::path::Path;

use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme, Sizable, v_flex};
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

/// Opens a folder or file in the desktop's default app.
pub fn open_path(path: &Path) {
	if let Err(e) = std::process::Command::new("xdg-open").arg(path).spawn() {
		tracing::warn!("could not open {}: {e}", path.display());
	}
}

pub fn format_date(unix: u64) -> String {
	chrono::DateTime::from_timestamp(unix as i64, 0)
		.map(|t| {
			t.with_timezone(&chrono::Local)
				.format("%Y-%m-%d %H:%M")
				.to_string()
		})
		.unwrap_or_default()
}

/// `3 h 20 min`, or just minutes under an hour.
pub fn format_play_time(seconds: u64) -> String {
	let minutes = seconds / 60;
	if minutes < 60 {
		t!("time.minutes", m = minutes).into_owned()
	} else {
		t!("time.hours", h = minutes / 60, m = minutes % 60).into_owned()
	}
}

/// A tinted tag: solid ones on every row draw too much attention.
pub fn soft_tag(color: Hsla, text: String) -> Tag {
	Tag::custom(color.opacity(0.15), color, color.opacity(0.4))
		.small()
		.child(text)
}

/// `1.4 GB`, `350 MB`, `12 KB`, in decimal units like file managers show.
pub fn format_size(bytes: u64) -> String {
	let b = bytes as f64;
	match bytes {
		0..1_000 => format!("{bytes} B"),
		1_000..1_000_000 => format!("{:.0} KB", b / 1e3),
		1_000_000..1_000_000_000 => format!("{:.0} MB", b / 1e6),
		_ => format!("{:.1} GB", b / 1e9),
	}
}
