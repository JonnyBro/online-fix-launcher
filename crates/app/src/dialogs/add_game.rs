//! Adding a game from a folder, an archive or an exe: pick the program
//! that starts it, the name, where it unpacks and where its prefix lives.

use rust_i18n::t;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme, Disableable, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use onlinefix_launcher_core::fix::Vendor;
use onlinefix_launcher_core::{Command, Engine, InspectError, NewGame, Source, SourceKind, Unpack};

/// What an installer run already settled for the game.
#[derive(Clone)]
pub struct Preset {
	pub id: String,
	pub name: String,
	pub prefix: Option<PathBuf>,
}

use crate::theme::Mono;

enum Stage {
	Inspecting,
	/// The archive wants a password; `wrong` after a failed try.
	Password {
		wrong: bool,
	},
	Ready(Box<Source>),
	Failed(String),
}

pub struct AddGameDialog {
	engine: Rc<Engine>,
	path: PathBuf,
	request: u64,
	stage: Stage,
	exe: Option<PathBuf>,
	name: Entity<InputState>,
	into: Entity<InputState>,
	prefix: Entity<InputState>,
	password: Entity<InputState>,
	delete_archive: bool,
	preset: Option<Preset>,
	_subscriptions: Vec<Subscription>,
}

impl AddGameDialog {
	pub fn open(
		engine: Rc<Engine>,
		path: PathBuf,
		games_dir: PathBuf,
		preset: Option<Preset>,
		request: u64,
		window: &mut Window,
		cx: &mut App,
	) -> Entity<Self> {
		let form = cx.new(|cx| {
			let mut input = |placeholder: String, cx: &mut Context<Self>| {
				cx.new(|cx| InputState::new(window, cx).placeholder(placeholder))
			};
			let name = input(String::new(), cx);
			let into = input(String::new(), cx);
			let prefix = input(t!("add.prefix_default").into_owned(), cx);
			let password = input(t!("add.password_hint").into_owned(), cx);
			into.update(cx, |i, cx| {
				i.set_value(games_dir.display().to_string(), window, cx)
			});
			let redraw =
				|input: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>| {
					cx.subscribe_in(input, window, |_, _, event: &InputEvent, _, cx| {
						if matches!(event, InputEvent::Change) {
							cx.notify();
						}
					})
				};
			let submit_password = cx.subscribe_in(&password, window, |this, _, event, _, cx| {
				if matches!(event, InputEvent::PressEnter { .. }) {
					this.retry_password(cx);
				}
			});
			let subscriptions = vec![
				redraw(&name, window, cx),
				redraw(&into, window, cx),
				redraw(&prefix, window, cx),
				submit_password,
			];
			engine.send(Command::InspectSource {
				request,
				path: path.clone(),
				password: None,
			});
			Self {
				engine,
				path,
				request,
				stage: Stage::Inspecting,
				exe: None,
				name,
				into,
				prefix,
				password,
				delete_archive: false,
				preset,
				_subscriptions: subscriptions,
			}
		});
		let dialog_form = form.clone();
		window.open_dialog(cx, move |dialog, _, _| {
			dialog
				.title(t!("add.title").into_owned())
				.w(px(640.))
				.child(dialog_form.clone())
		});
		form
	}

	pub fn request(&self) -> u64 {
		self.request
	}

	pub fn inspected(
		&mut self,
		result: Result<Box<Source>, InspectError>,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		self.stage = match result {
			Ok(source) => {
				self.exe = source.pick.clone();
				let name = match &self.preset {
					Some(preset) => preset.name.clone(),
					None => source.name.clone(),
				};
				self.name.update(cx, |i, cx| i.set_value(name, window, cx));
				if let Some(prefix) = self.preset.as_ref().and_then(|p| p.prefix.clone()) {
					let text = prefix.display().to_string();
					self.prefix
						.update(cx, |i, cx| i.set_value(text, window, cx));
				}
				Stage::Ready(source)
			}
			Err(InspectError::NeedPassword) => {
				let wrong = matches!(self.stage, Stage::Password { .. });
				self.password.update(cx, |i, cx| i.focus(window, cx));
				Stage::Password { wrong }
			}
			Err(InspectError::Failed(e)) => Stage::Failed(e),
		};
		cx.notify();
	}

	fn retry_password(&mut self, cx: &mut Context<Self>) {
		let password = self.password.read(cx).value().to_string();
		if password.is_empty() {
			return;
		}
		self.engine.send(Command::InspectSource {
			request: self.request,
			path: self.path.clone(),
			password: Some(password),
		});
		cx.notify();
	}

	fn pick_folder(&self, target: Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) {
		let picked = cx.prompt_for_paths(PathPromptOptions {
			files: false,
			directories: true,
			multiple: false,
			prompt: None,
		});
		cx.spawn_in(window, async move |_, cx| {
			if let Ok(Ok(Some(paths))) = picked.await
				&& let Some(path) = paths.into_iter().next()
			{
				let _ = cx.update(|window, cx| {
					target.update(cx, |i, cx| {
						i.set_value(path.display().to_string(), window, cx)
					});
				});
			}
		})
		.detach();
	}

	fn installing(&self, source: &Source) -> bool {
		self.exe
			.as_ref()
			.is_some_and(|exe| source.installers.contains(exe))
	}

	/// A folder named after the game inside the chosen one.
	fn named_folder(&self, source: &Source, cx: &App) -> PathBuf {
		let base = PathBuf::from(self.into.read(cx).value().trim());
		let folder = sanitize(self.name.read(cx).value().trim());
		base.join(if folder.is_empty() {
			source.name.clone()
		} else {
			folder
		})
	}

	/// The game's folder once unpacked: the archive's own top folder, or
	/// one named after the game.
	fn unpack_target(&self, source: &Source, cx: &App) -> (PathBuf, PathBuf) {
		let base = PathBuf::from(self.into.read(cx).value().trim());
		match &source.top_dir {
			Some(top) => (base.clone(), base.join(top)),
			None => {
				let into = self.named_folder(source, cx);
				(into.clone(), into)
			}
		}
	}

	fn submit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let Stage::Ready(source) = &self.stage else {
			return;
		};
		let Some(exe) = self.exe.clone() else {
			return;
		};
		let name = self.name.read(cx).value().trim().to_owned();
		let prefix = self.prefix.read(cx).value().trim().to_owned();
		let unpack = (source.kind == SourceKind::Archive).then(|| {
			let (into, game_dir) = self.unpack_target(source, cx);
			Unpack {
				archive: source.path.clone(),
				password: source.password.clone().unwrap_or_default(),
				into,
				game_dir,
				delete_archive: self.delete_archive,
			}
		});
		let install_into = self
			.installing(source)
			.then(|| self.named_folder(source, cx));
		self.engine.send(Command::AddGame(NewGame {
			id: self.preset.as_ref().map(|p| p.id.clone()),
			name,
			exe,
			prefix: (!prefix.is_empty()).then(|| PathBuf::from(prefix)),
			unpack,
			install_into,
		}));
		window.close_dialog(cx);
	}

	fn can_submit(&self, cx: &App) -> bool {
		let Stage::Ready(source) = &self.stage else {
			return false;
		};
		let needs_folder = source.kind == SourceKind::Archive || self.installing(source);
		let unpack_ok = !needs_folder || Path::new(self.into.read(cx).value().trim()).is_absolute();
		self.exe.is_some() && !self.name.read(cx).value().trim().is_empty() && unpack_ok
	}

	fn candidates(&self, source: &Source, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let root = match source.kind {
			SourceKind::Folder => source.path.clone(),
			_ => PathBuf::new(),
		};
		let rows = source.candidates.iter().enumerate().map(|(ix, path)| {
			let selected = self.exe.as_ref() == Some(path);
			let shown = path
				.strip_prefix(&root)
				.unwrap_or(path)
				.display()
				.to_string();
			let path = path.clone();
			div()
				.id(("candidate", ix))
				.px_3()
				.py_1()
				.rounded(theme.radius)
				.cursor_pointer()
				.when(selected, |d| {
					d.bg(theme.list_active).text_color(theme.foreground)
				})
				.hover(|d| d.bg(theme.list_hover))
				.mono(cx)
				.text_sm()
				.truncate()
				.child(shown)
				.on_click(cx.listener(move |this, _, _, cx| {
					this.exe = Some(path.clone());
					cx.notify();
				}))
		});
		div()
			.id("candidates")
			.max_h(px(180.))
			.overflow_y_scroll()
			.p_1()
			.rounded(theme.radius)
			.border_1()
			.border_color(theme.border)
			.children(rows)
	}

	fn ready(&self, source: &Source, cx: &mut Context<Self>) -> AnyElement {
		let theme = cx.theme().clone();
		let label = |text: String| {
			div()
				.text_sm()
				.text_color(theme.muted_foreground)
				.child(text)
		};
		let installing = self.installing(source);
		let archive = source.kind == SourceKind::Archive;
		let needs_folder = archive || installing;
		let fix_note = match &source.fix {
			Some(fix) if fix.is_empty() => Some((t!("add.no_fix").into_owned(), theme.warning)),
			Some(fix) => {
				let vendor = match fix.vendor {
					Some(Vendor::OnlineFix) => "OnlineFix",
					Some(Vendor::FreeTp) => "FreeTP",
					_ => "?",
				};
				Some((
					t!("add.fix_found", vendor = vendor, dlls = fix.overrides.len()).into_owned(),
					theme.success,
				))
			}
			None => None,
		};
		let into_input = self.into.clone();
		let prefix_input = self.prefix.clone();
		let target = if installing {
			Some(self.named_folder(source, cx))
		} else {
			archive.then(|| self.unpack_target(source, cx).1)
		};
		v_flex()
			.gap_3()
			.child(
				div()
					.text_sm()
					.text_color(theme.muted_foreground)
					.mono(cx)
					.truncate()
					.child(source.path.display().to_string()),
			)
			.children(fix_note.map(|(text, color)| div().text_sm().text_color(color).child(text)))
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("add.exe").into_owned()))
					.child(if source.candidates.is_empty() {
						div()
							.text_sm()
							.text_color(theme.danger)
							.child(t!("add.no_exe").into_owned())
							.into_any_element()
					} else {
						self.candidates(source, cx).into_any_element()
					}),
			)
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("game.name").into_owned()))
					.child(Input::new(&self.name).small()),
			)
			.when(installing, |form| {
				form.child(
					div()
						.text_sm()
						.text_color(theme.warning)
						.child(t!("add.freetp_installer").into_owned()),
				)
			})
			.when(needs_folder, |form| {
				form.child(
					v_flex()
						.gap_1()
						.child(label(if installing {
							t!("add.install_into").into_owned()
						} else {
							t!("add.unpack_into").into_owned()
						}))
						.child(
							h_flex()
								.gap_2()
								.child(div().flex_1().child(Input::new(&self.into).small()))
								.child(
									Button::new("into-browse")
										.label(t!("common.browse"))
										.small()
										.outline()
										.on_click(cx.listener(move |this, _, window, cx| {
											this.pick_folder(into_input.clone(), window, cx)
										})),
								),
						)
						.children(target.map(|t| {
							div()
								.text_sm()
								.text_color(theme.muted_foreground)
								.mono(cx)
								.truncate()
								.child(t!("add.game_folder", path = t.display()).into_owned())
						}))
						.when(archive, |col| {
							col.child(
								Checkbox::new("delete-archive")
									.label(t!("add.delete_archive").into_owned())
									.checked(self.delete_archive)
									.on_click(cx.listener(|this, checked: &bool, _, cx| {
										this.delete_archive = *checked;
										cx.notify();
									})),
							)
						}),
				)
			})
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("game.prefix").into_owned()))
					.child(
						h_flex()
							.gap_2()
							.child(div().flex_1().child(Input::new(&self.prefix).small()))
							.child(
								Button::new("prefix-browse")
									.label(t!("common.browse"))
									.small()
									.outline()
									.on_click(cx.listener(move |this, _, window, cx| {
										this.pick_folder(prefix_input.clone(), window, cx)
									})),
							),
					),
			)
			.into_any_element()
	}
}

/// Strips characters a folder name can't have.
fn sanitize(name: &str) -> String {
	name.chars()
		.filter(|c| !matches!(c, '/' | '\0'))
		.collect::<String>()
		.trim_matches('.')
		.trim()
		.to_owned()
}

impl Render for AddGameDialog {
	fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let note = |text: String, color: Hsla| div().text_sm().text_color(color).child(text);
		let body = match &self.stage {
			Stage::Inspecting => {
				note(t!("add.inspecting").into_owned(), theme.muted_foreground).into_any_element()
			}
			Stage::Failed(e) => note(e.clone(), theme.danger).into_any_element(),
			Stage::Password { wrong } => v_flex()
				.gap_2()
				.child(div().child(t!("add.password_needed").into_owned()))
				.when(*wrong, |c| {
					c.child(note(t!("add.password_wrong").into_owned(), theme.danger))
				})
				.child(
					h_flex()
						.gap_2()
						.child(div().flex_1().child(Input::new(&self.password).small()))
						.child(
							Button::new("password-ok")
								.label(t!("add.password_try"))
								.small()
								.primary()
								.on_click(cx.listener(|this, _, _, cx| this.retry_password(cx))),
						),
				)
				.into_any_element(),
			Stage::Ready(source) => {
				let source = source.clone();
				self.ready(&source, cx)
			}
		};
		let ready = matches!(self.stage, Stage::Ready(_));
		let can_submit = self.can_submit(cx);
		v_flex().gap_4().child(body).child(
			h_flex()
				.justify_end()
				.gap_2()
				.child(
					Button::new("add-cancel")
						.label(t!("common.cancel"))
						.outline()
						.on_click(|_, window, cx| window.close_dialog(cx)),
				)
				.when(ready, |row| {
					row.child(
						Button::new("add-ok")
							.label(t!("add.confirm"))
							.primary()
							.disabled(!can_submit)
							.on_click(cx.listener(|this, _, window, cx| this.submit(window, cx))),
					)
				}),
		)
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	// gpui's prelude has its own `test` attribute.
	use ::core::prelude::v1::test;

	#[test]
	fn folder_names() {
		assert_eq!(sanitize(" Cool/Game "), "CoolGame");
		assert_eq!(sanitize("..."), "");
	}
}
