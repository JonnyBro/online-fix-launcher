rust_i18n::i18n!("locales", fallback = "en");

pub mod app_update;
pub mod engine;
pub mod error;
mod http;
pub mod paths;
pub mod settings;

pub use engine::{Command, Engine, Event};
pub use error::{Error, Result};
pub use paths::AppPaths;
pub use settings::Settings;
