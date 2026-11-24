//! "Check everything": what's ready and what to do about what isn't.

use rust_i18n::t;

use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use onlinefix_launcher_core::checkup::{Check, Level};

use crate::widgets::soft_tag;

pub fn open(checks: Vec<Check>, window: &mut Window, cx: &mut App) {
	window.open_dialog(cx, move |dialog, _, cx| {
		let theme = cx.theme();
		let rows = checks.iter().enumerate().map(|(ix, check)| {
			let (label, color) = match check.level {
				Level::Ok => (t!("chk.ok"), theme.success),
				Level::Warning => (t!("chk.warning"), theme.warning),
				Level::Problem => (t!("chk.problem"), theme.danger),
				Level::Skipped => (t!("chk.skipped"), theme.muted_foreground),
			};
			h_flex()
				.gap_3()
				.items_start()
				.child(
					h_flex()
						.w(px(110.))
						.flex_shrink_0()
						.child(soft_tag(color, label.into_owned())),
				)
				.child(
					v_flex()
						.min_w_0()
						.child(div().child(check.title.clone()))
						.when(!check.detail.is_empty(), |d| {
							d.child(
								div()
									.text_sm()
									.text_color(theme.muted_foreground)
									.child(check.detail.clone()),
							)
						})
						.children(check.link.clone().map(|link| {
							Button::new(("check-link", ix))
								.label(t!("chk.fix"))
								.link()
								.xsmall()
								.on_click(move |_, _, cx| cx.open_url(&link))
						})),
				)
		});
		dialog
			.title(t!("chk.title").into_owned())
			.w(px(600.))
			.child(v_flex().gap_3().children(rows))
			.footer(
				h_flex().w_full().justify_end().child(
					Button::new("close")
						.label(t!("common.close"))
						.primary()
						.on_click(|_, window, cx| window.close_dialog(cx)),
				),
			)
	});
}
