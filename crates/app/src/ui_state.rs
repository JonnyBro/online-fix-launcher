//! What the window remembers between runs: its size, the open tab, view choices.

use std::path::Path;

use gpui_kit::*;
use serde::{Deserialize, Serialize};

use crate::shell::Tab;
use crate::views::library::LibraryPrefs;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SavedBounds {
	pub x: f32,
	pub y: f32,
	pub width: f32,
	pub height: f32,
	pub maximized: bool,
}

impl SavedBounds {
	pub fn from_window(bounds: WindowBounds) -> Self {
		let (b, maximized) = match bounds {
			WindowBounds::Windowed(b) => (b, false),
			WindowBounds::Maximized(b) | WindowBounds::Fullscreen(b) => (b, true),
		};
		Self {
			x: b.origin.x.as_f32(),
			y: b.origin.y.as_f32(),
			width: b.size.width.as_f32(),
			height: b.size.height.as_f32(),
			maximized,
		}
	}

	pub fn to_window(self) -> WindowBounds {
		let b = Bounds::new(
			point(px(self.x), px(self.y)),
			size(px(self.width), px(self.height)),
		);
		if self.maximized {
			WindowBounds::Maximized(b)
		} else {
			WindowBounds::Windowed(b)
		}
	}
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiState {
	pub window: Option<SavedBounds>,
	pub tab: Option<Tab>,
	pub logs_open: bool,
	/// Interface scale, 1.0 by default.
	pub scale: Option<f32>,
	pub library: LibraryPrefs,
}

impl UiState {
	pub fn load(path: &Path) -> Self {
		let Ok(text) = std::fs::read_to_string(path) else {
			return Self::default();
		};
		serde_json::from_str(&text).unwrap_or_else(|e| {
			tracing::warn!("{}: {e}", path.display());
			Self::default()
		})
	}

	pub fn save(&self, path: &Path) {
		let result = serde_json::to_string_pretty(self)
			.map_err(std::io::Error::other)
			.and_then(|text| std::fs::write(path, text));
		if let Err(e) = result {
			tracing::warn!("could not save {}: {e}", path.display());
		}
	}
}
