rust_i18n::i18n!("locales", fallback = "en");

pub mod app_update;
pub mod archive;
pub mod art;
pub mod checkup;
pub mod detect;
pub mod engine;
pub mod error;
pub mod fix;
mod http;
pub mod ini;
pub mod launch;
pub mod library;
pub mod patch;
pub mod paths;
pub mod proton;
pub mod proton_releases;
pub mod settings;
pub mod steam;
pub mod tools;
pub mod vdf;

pub use engine::{
	ArtOption, ArtSource, Command, Engine, Event, InspectError, NewGame, PendingStage, RunState,
	Source, SourceKind, Unpack,
};
pub use error::{Error, Result};
pub use paths::AppPaths;
pub use settings::Settings;
