use crate::app_update::AppRelease;
use crate::settings::Settings;

#[derive(Debug)]
pub enum Command {
	SaveSettings(Settings),
	CheckAppUpdate,
}

#[derive(Debug, Clone)]
pub enum Event {
	SettingsSaved(Settings),
	SettingsSaveFailed(String),
	AppUpdate(AppRelease),
	/// A short success message for the status area.
	Notice(String),
	Error(String),
}
