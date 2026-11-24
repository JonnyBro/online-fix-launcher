rust_i18n::i18n!("locales", fallback = "en");

pub mod app_update;
pub mod detect;
pub mod engine;
pub mod error;
pub mod fix;
mod http;
pub mod ini;
pub mod launch;
pub mod library;
pub mod paths;
pub mod proton;
pub mod settings;
pub mod steam;
pub mod vdf;

pub use engine::{Command, Engine, Event};
pub use error::{Error, Result};
pub use paths::AppPaths;
pub use settings::Settings;
