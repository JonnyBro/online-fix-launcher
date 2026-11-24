use super::*;

impl LibraryView {
	pub(super) fn grid(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		let theme = cx.theme().clone();
		let games: Vec<Game> = self.visible(cx).into_iter().cloned().collect();
		if games.is_empty() {
			return empty_state(
				t!("lib.no_match").into_owned(),
				t!("lib.no_match_hint").into_owned(),
				cx,
			)
			.into_any_element();
		}
		let cover = (CARD_WIDTH - 16., (CARD_WIDTH - 16.) * 1.5);
		let cards = games.into_iter().map(|game| {
			let selected = self.selected.as_deref() == Some(&game.id);
			let run = self.runs.get(&game.id).copied();
			let meta = match run {
				Some(state) => run_label(state),
				None if game.play_seconds > 0 => format_play_time(game.play_seconds),
				None => t!("lib.never_played").into_owned(),
			};
			let id = game.id.clone();
			let play_id = game.id.clone();
			v_flex()
				.id(SharedString::from(format!("card-{}", game.id)))
				.w(px(CARD_WIDTH))
				.p_2()
				.gap_1()
				.rounded(theme.radius_lg)
				.border_1()
				.border_color(if selected {
					theme.list_active_border
				} else {
					theme.border
				})
				.bg(if selected {
					theme.list_active
				} else {
					theme.muted
				})
				.hover(|s| s.bg(theme.list_hover))
				.cursor_pointer()
				.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
					if event.click_count() >= 2 {
						this.launch(&play_id);
					} else {
						let again = this.selected.as_deref() == Some(id.as_str());
						this.select((!again).then(|| id.clone()), window, cx);
					}
				}))
				.child(placeholder_cover(&game, cover, &theme))
				.child(div().truncate().child(game.name.clone()))
				.child(
					div()
						.text_sm()
						.truncate()
						.text_color(if run.is_some() {
							theme.success
						} else {
							theme.muted_foreground
						})
						.child(meta),
				)
		});
		div()
			.id("library-grid")
			.size_full()
			.overflow_y_scroll()
			.child(
				h_flex()
					.flex_wrap()
					.items_start()
					.gap(px(CARD_GAP))
					.p_5()
					.children(cards),
			)
			.into_any_element()
	}
}

pub(super) fn run_label(state: RunState) -> String {
	match state {
		RunState::WaitingForSteam => t!("run.waiting_steam"),
		RunState::Running => t!("run.running"),
		RunState::Stopping => t!("run.stopping"),
		RunState::Stopped => t!("run.stopped"),
	}
	.into_owned()
}
