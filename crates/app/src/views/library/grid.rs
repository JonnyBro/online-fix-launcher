use super::*;

impl LibraryView {
	pub(super) fn grid(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
		let theme = cx.theme().clone();
		let games: Vec<Game> = self.visible(cx).into_iter().cloned().collect();
		if games.is_empty() && self.unpacking.is_empty() {
			return empty_state(
				t!("lib.no_match").into_owned(),
				t!("lib.no_match_hint").into_owned(),
				cx,
			)
			.into_any_element();
		}
		let size = (CARD_WIDTH - 16., (CARD_WIDTH - 16.) * 1.5);
		let pending = self.unpacking.iter().enumerate().map(|(ix, u)| {
			v_flex()
				.id(("unpacking", ix))
				.w(px(CARD_WIDTH))
				.p_2()
				.gap_1()
				.rounded(theme.radius_lg)
				.border_1()
				.border_dashed()
				.border_color(theme.border)
				.child(
					v_flex()
						.w(px(size.0))
						.h(px(size.1))
						.items_center()
						.justify_center()
						.gap_2()
						.px_3()
						.child(
							div()
								.text_sm()
								.text_color(theme.muted_foreground)
								.child(t!("lib.unpacking").into_owned()),
						)
						.child(
							Progress::new(("unpack-progress", ix))
								.value(u.progress * 100.)
								.w_full(),
						)
						.child(
							div()
								.text_sm()
								.mono(cx)
								.child(format!("{:.0}%", u.progress * 100.)),
						),
				)
				.child(div().truncate().child(u.name.clone()))
				.into_any_element()
		});
		let pending: Vec<AnyElement> = pending.collect();
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
				.child({
					let art = self.art.get(&game.id).cloned().unwrap_or_default();
					cover(&game, art.grid.as_ref(), art.icon.as_ref(), size, &theme)
				})
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
					.children(pending)
					.children(cards.map(IntoElement::into_any_element)),
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
