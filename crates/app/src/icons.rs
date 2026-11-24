use std::borrow::Cow;

use gpui_kit::assets::{Assets, IconName, icon_assets};
use gpui_kit::component::Icon;
use gpui_kit::{AssetSource, Result, SharedString};

// The default Assets embed only the icons gpui-component itself uses; every other
// Lucide icon the app shows has to be listed here or it renders blank.
icon_assets!(ExtraIcons, [Gamepad2, Wine]);

pub struct AppAssets;

impl AssetSource for AppAssets {
	fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
		match ExtraIcons.load(path)? {
			Some(bytes) => Ok(Some(bytes)),
			None => Assets.load(path),
		}
	}

	fn list(&self, path: &str) -> Result<Vec<SharedString>> {
		let mut paths = Assets.list(path)?;
		paths.extend(ExtraIcons.list(path)?);
		paths.sort();
		paths.dedup();
		Ok(paths)
	}
}

#[derive(Clone, Copy)]
pub enum AppIcon {
	Library,
	Proton,
	Logs,
	Settings,
}

impl AppIcon {
	fn name(self) -> IconName {
		match self {
			Self::Library => IconName::Gamepad2,
			Self::Proton => IconName::Wine,
			Self::Logs => IconName::FileText,
			Self::Settings => IconName::Settings,
		}
	}
}

impl From<AppIcon> for Icon {
	fn from(icon: AppIcon) -> Self {
		Icon::new(icon.name())
	}
}
