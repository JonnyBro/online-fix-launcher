rust_i18n::i18n!("locales", fallback = "en");

mod dialogs;
mod icons;
mod logbuf;
mod notify;
mod shell;
mod theme;
mod ui_state;
mod views;
mod widgets;

use gpui_kit::*;
use onlinefix_launcher_core::paths::APP_ID;
use onlinefix_launcher_core::settings::Language;
use onlinefix_launcher_core::{AppPaths, Settings};
use tracing_subscriber::prelude::*;

pub const APP_NAME: &str = "OnlineFix Launcher";

fn main() {
	let paths = match AppPaths::detect().and_then(|p| p.ensure_exist().map(|()| p)) {
		Ok(paths) => paths,
		Err(e) => fatal(&e.to_string()),
	};
	let logs = logbuf::LogBuffer::default();
	let _log_guard = init_logging(&paths, logs.clone());

	let ui = ui_state::UiState::load(&paths.ui_state_file());
	let scale = ui.scale.unwrap_or(1.);
	let (settings, load_error) = match Settings::load(&paths) {
		Ok(settings) => (settings, None),
		Err(e) => {
			tracing::error!("{e}");
			(Settings::default(), Some(e.to_string()))
		}
	};
	apply_language(settings.language);
	// Two copies would write the same game list and fight over prefixes.
	let _instance = match single_instance(&paths) {
		Some(lock) => lock,
		None => {
			rfd::MessageDialog::new()
				.set_level(rfd::MessageLevel::Info)
				.set_title(APP_NAME)
				.set_description(rust_i18n::t!("app.already_running").into_owned())
				.show();
			return;
		}
	};

	gpui_kit::application()
		.with_assets(icons::AppAssets)
		.run(move |cx: &mut App| {
			gpui_kit::init(cx);
			theme::install(cx);
			cx.set_global(theme::UiScale(scale));

			let options = WindowOptions {
				window_bounds: Some(ui.window.map(|b| b.to_window()).unwrap_or_else(|| {
					WindowBounds::Windowed(Bounds::centered(None, size(px(1200.), px(780.)), cx))
				})),
				window_min_size: Some(size(px(900.), px(560.))),
				titlebar: Some(TitlebarOptions {
					title: Some(APP_NAME.into()),
					..Default::default()
				}),
				app_id: Some(APP_ID.into()),
				..Default::default()
			};
			let for_theme = settings.clone();
			gpui_kit::open_window(options, cx, move |window, cx| {
				theme::apply(&for_theme, window, cx);
				cx.new(|cx| {
					shell::Shell::new(
						shell::Startup {
							paths,
							settings,
							load_error,
							ui,
							logs,
						},
						window,
						cx,
					)
				})
			})
			.expect("open window");
			cx.on_window_closed(|cx, _| cx.quit()).detach();
			cx.activate(true);
		});
}

/// The lock on the instance file, or `None` when another copy holds it.
/// A lock that can't be taken for another reason doesn't stop the launcher.
fn single_instance(paths: &AppPaths) -> Option<Option<std::fs::File>> {
	let path = paths.lock_file();
	let file = match std::fs::OpenOptions::new()
		.create(true)
		.truncate(false)
		.write(true)
		.open(&path)
	{
		Ok(file) => file,
		Err(e) => {
			eprintln!("instance lock {}: {e}", path.display());
			return Some(None);
		}
	};
	match file.try_lock() {
		Ok(()) => Some(Some(file)),
		Err(std::fs::TryLockError::WouldBlock) => None,
		Err(std::fs::TryLockError::Error(e)) => {
			eprintln!("instance lock {}: {e}", path.display());
			Some(None)
		}
	}
}

fn apply_language(language: Language) {
	let locale = match language {
		Language::English => "en",
		Language::Russian => "ru",
		Language::System => match sys_locale::get_locale() {
			Some(l) if l.starts_with("ru") => "ru",
			_ => "en",
		},
	};
	// One global locale for this crate, the core and gpui-component.
	rust_i18n::set_locale(locale);
}

/// Started from a menu there is no terminal, so the error also goes to a dialog.
fn fatal(message: &str) -> ! {
	eprintln!("fatal: {message}");
	rfd::MessageDialog::new()
		.set_level(rfd::MessageLevel::Error)
		.set_title(APP_NAME)
		.set_description(message)
		.show();
	std::process::exit(1);
}

/// Logs go to stderr, to the logs panel and to a daily file in the logs folder.
fn init_logging(
	paths: &AppPaths,
	buffer: logbuf::LogBuffer,
) -> Option<tracing_appender::non_blocking::WorkerGuard> {
	use tracing_subscriber::{EnvFilter, fmt};
	// zbus warns on every portal file dialog: the request object is gone
	// before it reads its properties, which the dialog doesn't need.
	let default = if cfg!(debug_assertions) {
		"info,onlinefix_launcher=debug,onlinefix_launcher_core=debug,wgpu_hal=warn,wgpu_core=warn,zbus=error"
	} else {
		"info,wgpu_hal=warn,wgpu_core=warn,zbus=error"
	};
	let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
	// tracing writes timestamps in UTC by default; these use the local time zone.
	let timer = fmt::time::ChronoLocal::rfc_3339();
	let stderr = fmt::layer()
		.with_timer(timer.clone())
		.with_writer(std::io::stderr);
	let in_app = fmt::layer()
		.with_timer(timer.clone())
		.with_ansi(false)
		.with_target(false)
		.with_writer(buffer);
	match tracing_appender::rolling::Builder::new()
		.rotation(tracing_appender::rolling::Rotation::DAILY)
		.filename_prefix(APP_ID)
		.filename_suffix("log")
		.max_log_files(7)
		.build(paths.logs_dir())
	{
		Ok(appender) => {
			let (writer, guard) = tracing_appender::non_blocking(appender);
			let file = fmt::layer()
				.with_timer(timer)
				.with_ansi(false)
				.with_writer(writer);
			tracing_subscriber::registry()
				.with(filter)
				.with(stderr)
				.with(in_app)
				.with(file)
				.init();
			Some(guard)
		}
		Err(e) => {
			tracing_subscriber::registry()
				.with(filter)
				.with(stderr)
				.with(in_app)
				.init();
			tracing::warn!("file logging disabled: {e}");
			None
		}
	}
}
