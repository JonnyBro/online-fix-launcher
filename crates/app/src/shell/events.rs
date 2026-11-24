use super::*;

impl Shell {
	/// A desktop notification, only while the window isn't focused.
	fn desktop_note(&self, title: String, body: String, window: &Window) {
		if self.settings.notifications && !window.is_window_active() {
			crate::notify::send(title, body);
		}
	}

	pub(super) fn drain_events(&mut self, window: &mut Window, cx: &mut Context<Self>) {
		let events: Vec<Event> = self.engine.poll_events().collect();
		for event in events {
			self.handle_event(event, window, cx);
		}
		cx.notify();
	}

	fn handle_event(&mut self, event: Event, window: &mut Window, cx: &mut Context<Self>) {
		match event {
			Event::SettingsSaved(settings) => {
				crate::apply_language(settings.language);
				crate::theme::apply(&settings, window, cx);
				let for_view = settings.clone();
				self.settings_view
					.update(cx, |s, cx| s.on_saved(for_view, cx));
				let for_protons = settings.clone();
				self.protons
					.update(cx, |p, cx| p.set_settings(for_protons, cx));
				self.settings = settings;
				window.refresh();
			}
			Event::SettingsSaveFailed(text) => self.notify(Notification::error(text), window, cx),
			Event::AppUpdate(release) => self.app_update = Some(release),
			Event::Library(games) => self
				.library
				.update(cx, |l, cx| l.set_games(games, window, cx)),
			Event::GameAdded(id) => {
				let name = self.library.read(cx).game_name(&id).unwrap_or_default();
				self.desktop_note(t!("note.added").into_owned(), name, window);
				self.tab = Tab::Library;
				self.library
					.update(cx, |l, cx| l.select(Some(id), window, cx));
			}
			Event::Protons(protons) => {
				let for_view = protons.clone();
				self.protons.update(cx, |p, cx| p.set_builds(for_view, cx));
				self.library.update(cx, |l, cx| l.set_protons(protons, cx));
			}
			Event::MissingRuntimes(ids) => self
				.protons
				.update(cx, |p, cx| p.set_missing_runtimes(ids, cx)),
			Event::ProtonReleases { provider, result } => self
				.protons
				.update(cx, |p, cx| p.set_releases(provider, result, cx)),
			Event::ProtonInstalling { tag, progress } => self
				.protons
				.update(cx, |p, cx| p.set_installing(tag, progress, cx)),
			Event::ProtonInstallEnded(tag) => {
				self.protons.update(cx, |p, cx| p.install_ended(&tag, cx))
			}
			Event::Run { id, state } => self.library.update(cx, |l, cx| l.set_run(id, state, cx)),
			Event::SourceInspected { request, result } => {
				if let Some(dialog) = self.add_dialog.as_ref().and_then(|d| d.upgrade())
					&& dialog.read(cx).request() == request
				{
					dialog.update(cx, |d, cx| d.inspected(result, window, cx));
				}
			}
			Event::Pending { id, name, stage } => self
				.library
				.update(cx, |l, cx| l.set_pending(id, name, stage, cx)),
			Event::PendingEnded(id) => self.library.update(cx, |l, cx| l.unpack_ended(&id, cx)),
			Event::Installed {
				id,
				name,
				folder,
				prefix,
			} => self.open_add(folder, Some(Preset { id, name, prefix }), window, cx),
			Event::ArtChanged(id) => self.library.update(cx, |l, cx| l.art_changed(id, cx)),
			Event::ArtChoices { request, result } => {
				if let Some(gallery) = self.gallery.as_ref().and_then(|g| g.upgrade())
					&& gallery.read(cx).request() == request
				{
					gallery.update(cx, |g, cx| g.set_options(result, cx));
				}
			}
			Event::SteamGridDbKey(result) => self
				.settings_view
				.update(cx, |s, cx| s.key_checked(result, cx)),
			Event::Checkup { checks, show } => {
				use onlinefix_launcher_core::checkup::Level;
				if show {
					crate::dialogs::checkup::open(checks, window, cx);
				} else if checks.iter().any(|c| c.level == Level::Problem) {
					let note = Notification::warning(t!("chk.problems_found").into_owned())
						.on_click(move |_, window, cx| {
							crate::dialogs::checkup::open(checks.clone(), window, cx)
						});
					window.push_notification(note, cx);
				}
			}
			Event::Notice(text) => {
				self.desktop_note(crate::APP_NAME.to_owned(), text.clone(), window);
				self.notify(Notification::info(text), window, cx)
			}
			Event::Error(text) => {
				self.desktop_note(t!("note.error").into_owned(), text.clone(), window);
				self.notify(Notification::error(text), window, cx)
			}
		}
	}
}
