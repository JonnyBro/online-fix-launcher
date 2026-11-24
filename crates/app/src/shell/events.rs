use super::*;

impl Shell {
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
				self.settings = settings;
				window.refresh();
			}
			Event::SettingsSaveFailed(text) => self.notify(Notification::error(text), window, cx),
			Event::AppUpdate(release) => self.app_update = Some(release),
			Event::Notice(text) => self.notify(Notification::info(text), window, cx),
			Event::Error(text) => self.notify(Notification::error(text), window, cx),
		}
	}
}
