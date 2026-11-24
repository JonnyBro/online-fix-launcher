use rust_i18n::t;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants};
use gpui_kit::component::progress::Progress;
use gpui_kit::component::{
	ActiveTheme, Disableable, Icon, IconName, Selectable, Sizable, WindowExt, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use onlinefix_launcher_core::proton::{ProtonBuild, Source};
use onlinefix_launcher_core::proton_releases::{Provider, Release};
use onlinefix_launcher_core::{Command, Engine, Settings};

use crate::theme::Mono;
use crate::widgets::open_path;

const RELEASES_SHOWN: usize = 15;

/// Proton builds on this computer, and releases to download.
pub struct ProtonsView {
	engine: Rc<Engine>,
	folder: PathBuf,
	settings: Settings,
	builds: Vec<ProtonBuild>,
	missing_runtimes: Vec<u32>,
	provider: Provider,
	/// Missing until asked for, which happens when a source first shows.
	releases: HashMap<Provider, Result<Vec<Release>, String>>,
	loading: Vec<Provider>,
	/// Tag to download progress; `None` while unpacking.
	installing: HashMap<String, Option<f32>>,
}

impl ProtonsView {
	pub fn new(engine: Rc<Engine>, folder: PathBuf, settings: Settings) -> Self {
		Self {
			engine,
			folder,
			settings,
			builds: Vec::new(),
			missing_runtimes: Vec::new(),
			provider: Provider::GeProton,
			releases: HashMap::new(),
			loading: Vec::new(),
			installing: HashMap::new(),
		}
	}

	/// A source's releases are fetched once, when it's first shown.
	pub fn shown(&mut self, cx: &mut Context<Self>) {
		if !self.releases.contains_key(&self.provider) && !self.loading.contains(&self.provider) {
			self.reload(cx);
		}
	}

	fn reload(&mut self, cx: &mut Context<Self>) {
		if !self.loading.contains(&self.provider) {
			self.loading.push(self.provider);
		}
		self.engine.send(Command::ListProtonReleases(self.provider));
		self.engine.send(Command::RefreshProtons);
		cx.notify();
	}

	fn set_provider(&mut self, provider: Provider, cx: &mut Context<Self>) {
		self.provider = provider;
		self.shown(cx);
		cx.notify();
	}

	pub fn set_settings(&mut self, settings: Settings, cx: &mut Context<Self>) {
		self.settings = settings;
		cx.notify();
	}

	pub fn set_builds(&mut self, builds: Vec<ProtonBuild>, cx: &mut Context<Self>) {
		self.builds = builds;
		cx.notify();
	}

	pub fn set_missing_runtimes(&mut self, ids: Vec<u32>, cx: &mut Context<Self>) {
		self.missing_runtimes = ids;
		cx.notify();
	}

	pub fn set_releases(
		&mut self,
		provider: Provider,
		releases: Result<Vec<Release>, String>,
		cx: &mut Context<Self>,
	) {
		self.loading.retain(|p| *p != provider);
		self.releases.insert(provider, releases);
		cx.notify();
	}

	pub fn set_installing(&mut self, tag: String, progress: Option<f32>, cx: &mut Context<Self>) {
		self.installing.insert(tag, progress);
		cx.notify();
	}

	pub fn install_ended(&mut self, tag: &str, cx: &mut Context<Self>) {
		self.installing.remove(tag);
		cx.notify();
	}

	fn set_default(&mut self, dir: Option<PathBuf>) {
		let mut settings = self.settings.clone();
		settings.default_proton = dir;
		self.engine.send(Command::SaveSettings(settings));
	}

	fn confirm_remove(&self, build: &ProtonBuild, window: &mut Window, cx: &mut Context<Self>) {
		let engine = self.engine.clone();
		let dir = build.dir.clone();
		let text = t!("proton.remove_text", name = build.name).into_owned();
		window.open_alert_dialog(cx, move |alert, _, _| {
			let engine = engine.clone();
			let dir = dir.clone();
			alert
				.title(t!("proton.remove_title").into_owned())
				.description(text.clone())
				.confirm()
				.ok_text(t!("game.remove").into_owned())
				.cancel_text(t!("common.cancel").into_owned())
				.ok_variant(ButtonVariant::Danger)
				.on_ok(move |_, _, _| {
					engine.send(Command::RemoveProton(dir.clone()));
					true
				})
		});
	}

	fn installed(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let default = self.settings.default_proton.clone();
		let newest = self.builds.first().map(|b| b.name.clone());
		let auto = Button::new("default-auto")
			.label(match &newest {
				Some(name) => t!("proton.auto_is", name = name),
				None => t!("proton.auto"),
			})
			.small()
			.ghost()
			.selected(default.is_none())
			.on_click(cx.listener(|this, _, _, _| this.set_default(None)));
		let rows = self.builds.iter().enumerate().map(|(ix, build)| {
			let is_default = default.as_ref() == Some(&build.dir);
			let source = match build.source {
				Source::Launcher => t!("proton.src_launcher"),
				Source::CompatTools => t!("proton.src_compat"),
				Source::Steam => t!("proton.src_steam"),
			}
			.into_owned();
			let runtime_missing = build
				.runtime_app_id
				.filter(|id| self.missing_runtimes.contains(id));
			let dir = build.dir.clone();
			let for_remove = build.clone();
			h_flex()
				.id(("build", ix))
				.gap_3()
				.px_4()
				.py_2()
				.border_b_1()
				.border_color(theme.border.opacity(0.6))
				.child(
					v_flex()
						.flex_1()
						.min_w_0()
						.child(
							h_flex()
								.gap_2()
								.child(div().child(build.name.clone()))
								.child(
									div()
										.text_sm()
										.text_color(theme.muted_foreground)
										.child(source),
								),
						)
						.child(
							div()
								.text_sm()
								.text_color(theme.muted_foreground)
								.mono(cx)
								.truncate()
								.child(build.dir.display().to_string()),
						)
						.children(runtime_missing.map(|id| {
							h_flex()
								.gap_2()
								.text_sm()
								.child(
									div()
										.text_color(theme.warning)
										.child(t!("proton.runtime_missing").into_owned()),
								)
								.child(
									Button::new(("install-runtime", ix))
										.label(t!("proton.install_runtime"))
										.link()
										.xsmall()
										.on_click(move |_, _, cx| {
											cx.open_url(&format!("steam://install/{id}"))
										}),
								)
						})),
				)
				.child(
					Button::new(("make-default", ix))
						.label(if is_default {
							t!("proton.is_default")
						} else {
							t!("proton.make_default")
						})
						.small()
						.outline()
						.selected(is_default)
						.on_click(
							cx.listener(move |this, _, _, _| this.set_default(Some(dir.clone()))),
						),
				)
				.when(build.source == Source::Launcher, |row| {
					row.child(
						Button::new(("remove-build", ix))
							.icon(Icon::new(IconName::Delete))
							.small()
							.ghost()
							.tooltip(t!("game.remove"))
							.on_click(cx.listener(move |this, _, window, cx| {
								this.confirm_remove(&for_remove, window, cx)
							})),
					)
				})
		});
		v_flex()
			.child(
				h_flex()
					.px_4()
					.py_2()
					.gap_2()
					.child(
						div()
							.text_color(theme.muted_foreground)
							.child(t!("proton.default").into_owned()),
					)
					.child(auto),
			)
			.when(self.builds.is_empty(), |list| {
				list.child(
					div()
						.px_4()
						.py_3()
						.text_color(theme.muted_foreground)
						.child(t!("proton.none_installed").into_owned()),
				)
			})
			.children(rows)
	}

	fn releases(&self, cx: &mut Context<Self>) -> AnyElement {
		let theme = cx.theme().clone();
		let note = |text: String, color: Hsla| {
			div()
				.px_4()
				.py_3()
				.text_color(color)
				.child(text)
				.into_any_element()
		};
		let releases = match self.releases.get(&self.provider) {
			None => return note(t!("proton.loading").into_owned(), theme.muted_foreground),
			Some(Err(e)) => {
				return note(
					t!("proton.list_failed", error = e).into_owned(),
					theme.danger,
				);
			}
			Some(Ok(releases)) => releases,
		};
		let rows = releases
			.iter()
			.take(RELEASES_SHOWN)
			.enumerate()
			.map(|(ix, release)| {
				let installed = self.builds.iter().any(|b| {
					b.name == release.dir_name
						|| (release.provider == Provider::GeProton
							&& b.version.as_deref() == Some(&release.tag))
				});
				let progress = self.installing.get(&release.tag).copied();
				let action = match progress {
					Some(Some(p)) => h_flex()
						.gap_2()
						.w(px(180.))
						.child(Progress::new(("dl", ix)).value(p * 100.).flex_1())
						.child(div().text_sm().mono(cx).child(format!("{:.0}%", p * 100.)))
						.into_any_element(),
					Some(None) => div()
						.text_sm()
						.text_color(theme.muted_foreground)
						.child(t!("proton.unpacking").into_owned())
						.into_any_element(),
					None if installed => div()
						.text_sm()
						.text_color(theme.success)
						.child(t!("proton.installed_tag").into_owned())
						.into_any_element(),
					None => {
						let release = release.clone();
						Button::new(("install", ix))
							.label(t!("proton.install"))
							.icon(Icon::new(IconName::ArrowDown))
							.small()
							.outline()
							.on_click(cx.listener(move |this, _, _, cx| {
								this.engine.send(Command::InstallProton(release.clone()));
								this.installing.insert(release.tag.clone(), Some(0.));
								cx.notify();
							}))
							.into_any_element()
					}
				};
				h_flex()
					.gap_3()
					.px_4()
					.py_2()
					.border_b_1()
					.border_color(theme.border.opacity(0.6))
					.child(div().flex_1().child(release.tag.clone()))
					.child(
						div()
							.w(px(90.))
							.text_sm()
							.text_color(theme.muted_foreground)
							.mono(cx)
							.child(release.date.clone()),
					)
					.child(
						div()
							.w(px(80.))
							.text_sm()
							.text_color(theme.muted_foreground)
							.mono(cx)
							.child(format!("{:.0} MB", release.size as f64 / 1_000_000.)),
					)
					.child(div().w(px(180.)).flex().justify_end().child(action))
			});
		v_flex().children(rows).into_any_element()
	}
}

impl ProtonsView {
	fn provider_switch(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let hint = match self.provider {
			Provider::GeProton => t!("proton.ge_hint").into_owned(),
			Provider::CachyOs if onlinefix_launcher_core::proton_releases::cpu_has_v3() => {
				t!("proton.cachy_hint_v3").into_owned()
			}
			Provider::CachyOs => t!("proton.cachy_hint").into_owned(),
		};
		v_flex()
			.px_4()
			.pb_2()
			.gap_2()
			.child(
				h_flex()
					.gap_1()
					.children(Provider::ALL.into_iter().enumerate().map(|(ix, provider)| {
						Button::new(("provider", ix))
							.label(provider.label())
							.small()
							.ghost()
							.selected(self.provider == provider)
							.on_click(
								cx.listener(move |this, _, _, cx| this.set_provider(provider, cx)),
							)
					})),
			)
			.child(
				div()
					.text_sm()
					.text_color(cx.theme().muted_foreground)
					.child(hint),
			)
	}
}

fn section(title: String, cx: &App) -> Div {
	div()
		.px_4()
		.pt_5()
		.pb_2()
		.text_color(cx.theme().muted_foreground)
		.child(title)
}

impl Render for ProtonsView {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let folder = self.folder.clone();
		v_flex()
			.size_full()
			.child(
				h_flex()
					.px_5()
					.pt_4()
					.pb_2()
					.gap_3()
					.child(div().text_xl().child(t!("tab.proton").into_owned()))
					.child(div().flex_1())
					.child(
						Button::new("protons-folder")
							.label(t!("common.open_folder"))
							.icon(Icon::new(IconName::FolderOpen))
							.small()
							.outline()
							.on_click(move |_, _, _| {
								if let Err(e) = std::fs::create_dir_all(&folder) {
									tracing::warn!("{}: {e}", folder.display());
								}
								open_path(&folder);
							}),
					)
					.child(
						Button::new("protons-reload")
							.label(t!("proton.reload"))
							.icon(Icon::new(IconName::RefreshCw))
							.small()
							.outline()
							.loading(self.loading.contains(&self.provider))
							.disabled(self.loading.contains(&self.provider))
							.on_click(cx.listener(|this, _, _, cx| this.reload(cx))),
					),
			)
			.child(
				div()
					.id("protons-body")
					.flex_1()
					.min_h_0()
					.overflow_y_scroll()
					.border_t_1()
					.border_color(cx.theme().border)
					.child(
						v_flex()
							.max_w(px(900.))
							.pb_6()
							.child(section(t!("proton.installed_title").into_owned(), cx))
							.child(self.installed(cx))
							.child(section(t!("proton.download_title").into_owned(), cx))
							.child(self.provider_switch(cx))
							.child(self.releases(cx)),
					),
			)
	}
}
