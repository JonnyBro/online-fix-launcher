use super::grid::run_label;
use super::*;

impl LibraryView {
	pub(super) fn details(
		&self,
		_window: &mut Window,
		cx: &mut Context<Self>,
	) -> Option<AnyElement> {
		let game = self.draft.clone()?;
		let saved = self.saved()?.clone();
		let theme = cx.theme().clone();
		let run = self.runs.get(&game.id).copied();
		let id = game.id.clone();
		let cover_w = DETAILS_WIDTH - 32.;

		let play = match run {
			None => Button::new("play")
				.label(t!("game.play"))
				.icon(Icon::new(IconName::Play))
				.primary()
				.on_click(cx.listener({
					let id = id.clone();
					move |this, _, _, _| this.launch(&id)
				})),
			Some(RunState::Stopping) => Button::new("play")
				.label(run_label(RunState::Stopping))
				.outline()
				.loading(true)
				.disabled(true),
			Some(state) => Button::new("play")
				.label(t!("game.stop"))
				.icon(Icon::new(IconName::Square))
				.with_variant(ButtonVariant::Danger)
				.tooltip(run_label(state))
				.on_click(cx.listener({
					let id = id.clone();
					move |this, _, _, _| this.engine.send(Command::Stop(id.clone()))
				})),
		};

		let proton_name = match &saved.proton {
			Some(dir) => self
				.protons
				.iter()
				.find(|p| &p.dir == dir)
				.map(|p| p.name.clone())
				.unwrap_or_else(|| t!("game.proton_gone").into_owned()),
			None => match self.protons.first() {
				Some(p) => t!("game.proton_default_is", name = p.name).into_owned(),
				None => t!("game.proton_none").into_owned(),
			},
		};
		let mut facts: Vec<(String, String)> = vec![
			(
				t!("game.played").into(),
				if saved.play_seconds > 0 {
					format_play_time(saved.play_seconds)
				} else {
					t!("lib.never_played").into()
				},
			),
			(
				t!("game.last_played").into(),
				saved
					.last_played
					.map(format_date)
					.unwrap_or_else(|| "—".into()),
			),
			(t!("game.proton").into(), proton_name),
		];
		if let Some(app_id) = saved.real_app_id {
			facts.push((t!("game.app_id").into(), app_id.to_string()));
		}
		facts.push((
			t!("game.fix_overrides").into(),
			if saved.fix_overrides.is_empty() {
				t!("game.no_fix").into()
			} else {
				saved.fix_overrides.replace(';', "; ")
			},
		));

		let art_menu = {
			let view = cx.entity().downgrade();
			let id = game.id.clone();
			Button::new("art-menu")
				.label(t!("art.menu"))
				.icon(Icon::new(IconName::Palette))
				.small()
				.outline()
				.dropdown_menu(move |menu, _, _| {
					let pick = |label: String, kind: ArtKind| {
						let (view, id) = (view.clone(), id.clone());
						PopupMenuItem::new(label).on_click(move |_, _, cx| {
							let id = id.clone();
							let _ = view
								.update(cx, |_, cx| cx.emit(LibraryEvent::PickArt { id, kind }));
						})
					};
					let from_file = |label: String, kind: ArtKind| {
						let (view, id) = (view.clone(), id.clone());
						PopupMenuItem::new(label).on_click(move |_, window, cx| {
							let id = id.clone();
							let _ = view
								.update(cx, |this, cx| this.art_from_file(id, kind, window, cx));
						})
					};
					let engine_view = view.clone();
					let refetch_id = id.clone();
					menu.item(from_file(t!("art.cover_file").into_owned(), ArtKind::Grid))
						.item(from_file(t!("art.banner_file").into_owned(), ArtKind::Hero))
						.separator()
						.item(pick(t!("art.pick_cover").into_owned(), ArtKind::Grid))
						.item(pick(t!("art.pick_banner").into_owned(), ArtKind::Hero))
						.separator()
						.item(
							PopupMenuItem::new(t!("art.refetch")).on_click(move |_, _, cx| {
								let id = refetch_id.clone();
								let _ = engine_view
									.update(cx, |this, _| this.engine.send(Command::FetchArt(id)));
							}),
						)
				})
		};
		let tools_menu = {
			let view = cx.entity().downgrade();
			let id = game.id.clone();
			Button::new("tools-menu")
				.label(t!("tools.menu"))
				.icon(Icon::new(IconName::Settings2))
				.small()
				.outline()
				.disabled(run.is_some())
				.dropdown_menu(move |mut menu, _, _| {
					let rescan = {
						let (view, id) = (view.clone(), id.clone());
						PopupMenuItem::new(t!("tools.rescan_fix")).on_click(move |_, _, cx| {
							let id = id.clone();
							let _ =
								view.update(cx, |this, _| this.engine.send(Command::RescanFix(id)));
						})
					};
					menu = menu.item(rescan).separator();
					for tool in WineTool::ALL {
						let (view, id) = (view.clone(), id.clone());
						menu = menu.item(PopupMenuItem::new(tool_label(tool)).on_click(
							move |_, _, cx| {
								let id = id.clone();
								let _ = view.update(cx, |this, _| {
									this.engine.send(Command::RunTool { id, tool })
								});
							},
						));
					}
					menu
				})
		};
		let folders = {
			let game_dir = game.work_dir().to_path_buf();
			let prefix = game.prefix_dir(&self.paths);
			let log_id = game.id.clone();
			let log_open = self.log.as_ref().is_some_and(|log| log.id == game.id);
			h_flex()
				.gap_2()
				.flex_wrap()
				.child(art_menu)
				.child(tools_menu)
				.child(
					Button::new("open-game-dir")
						.label(t!("game.open_folder"))
						.icon(Icon::new(IconName::FolderOpen))
						.small()
						.outline()
						.on_click(move |_, _, _| open_path(&game_dir)),
				)
				.child(
					Button::new("open-prefix")
						.label(t!("game.open_prefix"))
						.small()
						.outline()
						.disabled(!prefix.is_dir())
						.on_click(move |_, _, _| open_path(&prefix)),
				)
				.child(
					Button::new("open-log")
						.label(t!("game.open_log"))
						.icon(Icon::new(IconName::SquareTerminal))
						.small()
						.outline()
						.selected(log_open)
						.on_click(cx.listener(move |this, _, window, cx| {
							if log_open {
								this.close_log(cx);
							} else {
								this.open_log(log_id.clone(), window, cx);
							}
						})),
				)
		};

		Some(
			v_flex()
				.w(px(DETAILS_WIDTH))
				.flex_shrink_0()
				.h_full()
				.border_l_1()
				.border_color(theme.border)
				.bg(theme.sidebar)
				.child(
					h_flex()
						.px_4()
						.pt_4()
						.gap_2()
						.child(div().flex_1().min_w_0().text_lg().child(saved.name.clone()))
						.child(
							Button::new("favorite")
								.icon(Icon::new(if saved.favorite {
									IconName::StarFill
								} else {
									IconName::Star
								}))
								.ghost()
								.xsmall()
								.when(saved.favorite, |b| b.text_color(theme.warning))
								.tooltip(if saved.favorite {
									t!("lib.unfavorite")
								} else {
									t!("lib.favorite")
								})
								.on_click(cx.listener({
									let (id, value) = (id.clone(), !saved.favorite);
									move |this, _, _, cx| this.set_favorite(&id, value, cx)
								})),
						)
						.child(
							Button::new("hide")
								.icon(Icon::new(if saved.hidden {
									IconName::Eye
								} else {
									IconName::EyeOff
								}))
								.ghost()
								.xsmall()
								.tooltip(if saved.hidden {
									t!("lib.unhide")
								} else {
									t!("lib.hide")
								})
								.on_click(cx.listener({
									let (id, value) = (id.clone(), !saved.hidden);
									move |this, _, window, cx| {
										this.set_hidden(&id, value, window, cx)
									}
								})),
						)
						.child(
							Button::new("close-details")
								.icon(Icon::new(IconName::Close))
								.ghost()
								.xsmall()
								.on_click(
									cx.listener(|this, _, window, cx| {
										this.select(None, window, cx)
									}),
								),
						),
				)
				.child(
					div()
						.id("details")
						.flex_1()
						.min_h_0()
						.overflow_y_scroll()
						.child(
							v_flex()
								.p_4()
								.gap_4()
								.child({
									let art = self.art.get(&saved.id).cloned().unwrap_or_default();
									let picture = art.hero.as_ref().or(art.grid.as_ref());
									cover(
										&saved,
										picture,
										art.icon.as_ref(),
										(cover_w, cover_w * 0.47),
										&theme,
									)
								})
								.child(h_flex().gap_2().child(play).children(run.map(|state| {
									div()
										.text_sm()
										.text_color(theme.success)
										.child(run_label(state))
								})))
								.child(v_flex().gap_1().text_sm().children(facts.into_iter().map(
									|(label, value)| {
										h_flex()
											.gap_3()
											.items_start()
											.child(
												div()
													.w(px(120.))
													.flex_shrink_0()
													.text_color(theme.muted_foreground)
													.child(label),
											)
											.child(div().min_w_0().mono(cx).child(value))
									},
								)))
								.child(folders)
								.child(self.saves_section(&game, cx))
								.child(self.settings_form(&game, cx))
								.child(
									div().pt_2().child(
										Button::new("remove-game")
											.label(t!("game.remove"))
											.icon(Icon::new(IconName::Delete))
											.small()
											.ghost()
											.with_variant(ButtonVariant::Danger)
											.disabled(run.is_some())
											.on_click(cx.listener({
												let game = saved.clone();
												move |this, _, window, cx| {
													this.confirm_remove(&game, window, cx)
												}
											})),
									),
								),
						),
				)
				.when(self.is_dirty(), |panel| panel.child(self.save_bar(cx)))
				.into_any_element(),
		)
	}

	fn settings_form(&self, game: &Game, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme();
		let label = |text: String| {
			div()
				.text_sm()
				.text_color(theme.muted_foreground)
				.child(text)
		};
		let toggle = |id: &'static str, text: String, checked: bool, apply: fn(&mut Game, bool)| {
			Checkbox::new(id)
				.label(text)
				.checked(checked)
				.on_click(cx.listener(move |this, checked: &bool, _, cx| {
					if let Some(draft) = &mut this.draft {
						apply(draft, *checked);
						cx.notify();
					}
				}))
		};
		let installed = self.installed;
		// A switch for a program that isn't installed says so; it still
		// works as a setting, the launch just stops with an error.
		let tool_toggle = |id: &'static str,
		                   text: String,
		                   present: bool,
		                   checked: bool,
		                   apply: fn(&mut Game, bool)| {
			let text = if present {
				text
			} else {
				t!("game.not_installed", name = text).into_owned()
			};
			toggle(id, text, checked, apply)
		};
		let size_pair = |caption: String, a: &Entity<InputState>, b: &Entity<InputState>| {
			h_flex()
				.gap_2()
				.child(
					div()
						.w(px(110.))
						.flex_shrink_0()
						.text_sm()
						.text_color(theme.muted_foreground)
						.child(caption),
				)
				.child(Input::new(a).small().w(px(80.)))
				.child(
					div()
						.text_sm()
						.text_color(theme.muted_foreground)
						.child("×"),
				)
				.child(Input::new(b).small().w(px(80.)))
		};
		let gamescope = game.gamescope.clone();
		let wrappers = v_flex()
			.gap_2()
			.child(label(t!("game.wrappers").into()))
			.child(tool_toggle(
				"mangohud",
				"MangoHud".into(),
				installed.mangohud,
				game.mangohud,
				|g, v| g.mangohud = v,
			))
			.child(tool_toggle(
				"gamemode",
				"GameMode".into(),
				installed.gamemode,
				game.gamemode,
				|g, v| g.gamemode = v,
			))
			.child(toggle(
				"nvapi",
				t!("game.nvapi").into(),
				game.nvapi,
				|g, v| g.nvapi = v,
			))
			.child(
				Checkbox::new("fsr")
					.label(t!("game.fsr").into_owned())
					.checked(game.fsr)
					.tooltip(if gamescope.enabled {
						t!("game.fsr_gamescope_hint")
					} else {
						t!("game.fsr_hint")
					})
					.on_click(cx.listener(|this, checked: &bool, _, cx| {
						if let Some(draft) = &mut this.draft {
							draft.fsr = *checked;
							cx.notify();
						}
					})),
			)
			.child(tool_toggle(
				"gamescope",
				"gamescope".into(),
				installed.gamescope,
				gamescope.enabled,
				|g, v| g.gamescope.enabled = v,
			))
			.when(gamescope.enabled, |section| {
				section.child(
					v_flex()
						.gap_2()
						.pl_6()
						.child(size_pair(
							t!("game.gs_output").into(),
							&self.fields.gs_width,
							&self.fields.gs_height,
						))
						.child(size_pair(
							t!("game.gs_game").into(),
							&self.fields.gs_game_width,
							&self.fields.gs_game_height,
						))
						.child(
							h_flex()
								.gap_2()
								.child(
									div()
										.w(px(110.))
										.flex_shrink_0()
										.text_sm()
										.text_color(theme.muted_foreground)
										.child(t!("game.gs_refresh").into_owned()),
								)
								.child(Input::new(&self.fields.gs_refresh).small().w(px(80.)))
								.child(
									div()
										.text_sm()
										.text_color(theme.muted_foreground)
										.child(t!("game.hz").into_owned()),
								),
						)
						.child(toggle(
							"gs-fullscreen",
							t!("game.gs_fullscreen").into(),
							gamescope.fullscreen,
							|g, v| g.gamescope.fullscreen = v,
						))
						.child(
							div()
								.text_xs()
								.text_color(theme.muted_foreground)
								.child(t!("game.gs_hint").into_owned()),
						),
				)
			});
		let current = game.proton.clone();
		let current_label = match &current {
			Some(dir) => self
				.protons
				.iter()
				.find(|p| &p.dir == dir)
				.map(|p| p.name.clone())
				.unwrap_or_else(|| dir.display().to_string()),
			None => t!("game.proton_default").into_owned(),
		};
		let protons = self.protons.clone();
		let view = cx.entity().downgrade();
		let proton_picker = Button::new("game-proton")
			.label(current_label)
			.small()
			.outline()
			.dropdown_caret(true)
			.dropdown_menu(move |mut menu, _, _| {
				let choice = |label: String, dir: Option<std::path::PathBuf>| {
					let view = view.clone();
					let checked = dir == current;
					PopupMenuItem::new(label)
						.checked(checked)
						.on_click(move |_, _, cx| {
							let dir = dir.clone();
							let _ = view.update(cx, |this, cx| {
								if let Some(draft) = &mut this.draft {
									draft.proton = dir;
									cx.notify();
								}
							});
						})
				};
				menu = menu.item(choice(t!("game.proton_default").into_owned(), None));
				for p in &protons {
					menu = menu.item(choice(p.name.clone(), Some(p.dir.clone())));
				}
				menu
			});
		v_flex()
			.gap_3()
			.pt_2()
			.border_t_1()
			.border_color(theme.border)
			.child(div().pt_2().child(t!("game.settings").into_owned()))
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("game.name").into()))
					.child(Input::new(&self.fields.name).small()),
			)
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("game.proton").into()))
					.child(proton_picker),
			)
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("game.args_after").into()))
					.child(Input::new(&self.fields.args_after).small()),
			)
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("game.args_before").into()))
					.child(Input::new(&self.fields.args_before).small()),
			)
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("game.prefix").into()))
					.child(Input::new(&self.fields.prefix).small()),
			)
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("game.extra_overrides").into()))
					.child(Input::new(&self.fields.overrides).small()),
			)
			.child(
				v_flex()
					.gap_1()
					.child(label(t!("game.env").into()))
					.child(Textarea::new(&self.fields.env).small().mono(cx))
					.child(
						div()
							.text_xs()
							.text_color(theme.muted_foreground)
							.child(t!("game.env_hint").into_owned()),
					),
			)
			.child(wrappers)
			.child(
				v_flex()
					.gap_2()
					.child(toggle(
						"steam-runtime",
						t!("game.steam_runtime").into(),
						game.steam_runtime,
						|g, v| g.steam_runtime = v,
					))
					.child(toggle(
						"steam-overlay",
						t!("game.steam_overlay").into(),
						game.steam_overlay,
						|g, v| g.steam_overlay = v,
					))
					.child(toggle(
						"wayland",
						t!("game.wayland").into(),
						game.wayland,
						|g, v| g.wayland = v,
					))
					.child(toggle(
						"wined3d",
						t!("game.wined3d").into(),
						game.wined3d,
						|g, v| g.wined3d = v,
					)),
			)
	}

	fn art_from_file(
		&self,
		id: String,
		kind: ArtKind,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let picked = cx.prompt_for_paths(PathPromptOptions {
			files: true,
			directories: false,
			multiple: false,
			prompt: Some(t!("art.pick_file").into()),
		});
		let engine = self.engine.clone();
		cx.spawn_in(window, async move |_, _| {
			if let Ok(Ok(Some(paths))) = picked.await
				&& let Some(file) = paths.into_iter().next()
			{
				engine.send(Command::SetArt {
					id,
					kind,
					from: onlinefix_launcher_core::ArtSource::File(file),
				});
			}
		})
		.detach();
	}

	fn save_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme();
		let can_save = self
			.draft
			.as_ref()
			.is_some_and(|g| !g.name.trim().is_empty());
		h_flex()
			.px_4()
			.py_3()
			.gap_2()
			.border_t_1()
			.border_color(theme.border)
			.bg(theme.muted)
			.child(
				div()
					.flex_1()
					.min_w_0()
					.truncate()
					.text_sm()
					.text_color(theme.muted_foreground)
					.child(t!("game.unsaved").into_owned()),
			)
			.child(
				Button::new("game-revert")
					.label(t!("game.revert"))
					.small()
					.outline()
					.on_click(cx.listener(|this, _, window, cx| {
						this.draft = this.saved().cloned();
						this.fill_fields(true, window, cx);
						cx.notify();
					})),
			)
			.child(
				Button::new("game-save")
					.label(t!("game.save"))
					.small()
					.primary()
					.disabled(!can_save)
					.on_click(cx.listener(|this, _, _, _| {
						if let Some(mut game) = this.draft.clone() {
							game.name = game.name.trim().to_owned();
							this.engine.send(Command::UpdateGame(Box::new(game)));
						}
					})),
			)
	}

	fn saves_section(&self, game: &Game, cx: &mut Context<Self>) -> impl IntoElement {
		let theme = cx.theme().clone();
		let running = self.runs.contains_key(&game.id);
		let backups = self.backups.get(&game.id).cloned().unwrap_or_default();
		let folder =
			onlinefix_launcher_core::saves::backups_dir(&self.paths.backups_dir(), &game.name);
		let id = game.id.clone();
		let rows = backups.into_iter().enumerate().map(|(ix, backup)| {
			let (restore_id, delete_id) = (id.clone(), id.clone());
			let (restore_path, delete_path) = (backup.path.clone(), backup.path.clone());
			let when = format_date(backup.created);
			h_flex()
				.gap_2()
				.text_sm()
				.child(div().flex_1().mono(cx).child(when.clone()))
				.child(
					div()
						.text_color(theme.muted_foreground)
						.mono(cx)
						.child(format_size(backup.size)),
				)
				.child(
					Button::new(("restore-backup", ix))
						.label(t!("saves.restore"))
						.xsmall()
						.outline()
						.disabled(running)
						.on_click(cx.listener(move |this, _, window, cx| {
							this.confirm_restore(
								restore_id.clone(),
								restore_path.clone(),
								when.clone(),
								window,
								cx,
							)
						})),
				)
				.child(
					Button::new(("delete-backup", ix))
						.icon(Icon::new(IconName::Delete))
						.xsmall()
						.ghost()
						.tooltip(t!("game.remove"))
						.on_click(cx.listener(move |this, _, _, _| {
							this.engine.send(Command::DeleteBackup {
								id: delete_id.clone(),
								backup: delete_path.clone(),
							})
						})),
				)
		});
		let rows: Vec<_> = rows.collect();
		v_flex()
			.gap_2()
			.pt_2()
			.border_t_1()
			.border_color(theme.border)
			.child(div().pt_2().child(t!("saves.title").into_owned()))
			.child(
				h_flex()
					.gap_2()
					.child(
						Button::new("back-up-saves-now")
							.label(t!("saves.back_up"))
							.small()
							.outline()
							.disabled(running)
							.on_click(cx.listener(move |this, _, _, _| {
								this.engine.send(Command::BackUpSaves(id.clone()))
							})),
					)
					.child(
						Button::new("backups-folder")
							.label(t!("common.open_folder"))
							.small()
							.ghost()
							.disabled(!folder.is_dir())
							.on_click(move |_, _, _| open_path(&folder)),
					),
			)
			.when(rows.is_empty(), |c| {
				c.child(
					div()
						.text_sm()
						.text_color(theme.muted_foreground)
						.child(t!("saves.none").into_owned()),
				)
			})
			.children(rows)
	}

	fn confirm_restore(
		&self,
		id: String,
		backup: std::path::PathBuf,
		when: String,
		window: &mut Window,
		cx: &mut Context<Self>,
	) {
		let engine = self.engine.clone();
		let text = t!("saves.restore_text", when = when).into_owned();
		window.open_alert_dialog(cx, move |alert, _, _| {
			let engine = engine.clone();
			let (id, backup) = (id.clone(), backup.clone());
			alert
				.title(t!("saves.restore_title").into_owned())
				.description(text.clone())
				.confirm()
				.ok_text(t!("saves.restore").into_owned())
				.cancel_text(t!("common.cancel").into_owned())
				.on_ok(move |_, _, _| {
					engine.send(Command::RestoreSaves {
						id: id.clone(),
						backup: backup.clone(),
					});
					true
				})
		});
	}

	fn confirm_remove(&self, game: &Game, window: &mut Window, cx: &mut Context<Self>) {
		let engine = self.engine.clone();
		let id = game.id.clone();
		let text = t!("game.remove_text", name = game.name).into_owned();
		let game_dir = fix::game_root(&game.exe);
		let prefix = game.prefix_dir(&self.paths);
		// The launcher's own prefix is useless without the game; one picked
		// by hand may be shared, so it stays unless asked.
		let delete_prefix = Rc::new(Cell::new(game.prefix.is_none()));
		let delete_game_dir = Rc::new(Cell::new(false));
		let back_up = Rc::new(Cell::new(true));
		window.open_dialog(cx, move |dialog, _, cx| {
			let muted = cx.theme().muted_foreground;
			let option =
				|id: &'static str, label: String, path: &std::path::Path, cell: &Rc<Cell<bool>>| {
					let cell = cell.clone();
					v_flex()
						.gap_1()
						.child(Checkbox::new(id).label(label).checked(cell.get()).on_click(
							move |checked, window, _| {
								cell.set(*checked);
								window.refresh();
							},
						))
						.child(
							div()
								.pl_6()
								.text_sm()
								.text_color(muted)
								.font_family(cx.theme().mono_font_family.clone())
								.child(path.display().to_string()),
						)
				};
			let engine = engine.clone();
			let id = id.clone();
			let (prefix_cell, dir_cell, backup_cell) = (
				delete_prefix.clone(),
				delete_game_dir.clone(),
				back_up.clone(),
			);
			let backup_toggle = {
				let cell = back_up.clone();
				div().pl_6().child(
					Checkbox::new("back-up-saves")
						.label(t!("game.back_up_first").into_owned())
						.checked(cell.get())
						.on_click(move |checked, window, _| {
							cell.set(*checked);
							window.refresh();
						}),
				)
			};
			let deleting = delete_game_dir.get();
			dialog
				.title(t!("game.remove_title").into_owned())
				.w(px(520.))
				.child(
					v_flex()
						.gap_3()
						.child(div().child(text.clone()))
						.child(option(
							"delete-prefix",
							t!("game.delete_prefix").into_owned(),
							&prefix,
							&delete_prefix,
						))
						.when(delete_prefix.get(), |list| list.child(backup_toggle))
						.child(option(
							"delete-game-dir",
							t!("game.delete_game_dir").into_owned(),
							&game_dir,
							&delete_game_dir,
						))
						.when(deleting, |list| {
							list.child(
								div()
									.text_sm()
									.text_color(cx.theme().danger)
									.child(t!("game.delete_game_dir_warning").into_owned()),
							)
						}),
				)
				.footer(
					h_flex()
						.w_full()
						.justify_end()
						.gap_2()
						.child(
							Button::new("remove-cancel")
								.label(t!("common.cancel"))
								.outline()
								.on_click(|_, window, cx| window.close_dialog(cx)),
						)
						.child(
							Button::new("remove-ok")
								.label(t!("game.remove"))
								.with_variant(ButtonVariant::Danger)
								.on_click(move |_, window, cx| {
									engine.send(Command::RemoveGame {
										id: id.clone(),
										delete_game_dir: dir_cell.get(),
										delete_prefix: prefix_cell.get(),
										back_up: backup_cell.get(),
									});
									window.close_dialog(cx);
								}),
						),
				)
		});
	}
}

fn tool_label(tool: WineTool) -> String {
	match tool {
		WineTool::Winecfg => t!("tools.winecfg"),
		WineTool::Winetricks => t!("tools.winetricks"),
		WineTool::Regedit => t!("tools.regedit"),
		WineTool::Taskmgr => t!("tools.taskmgr"),
		WineTool::Control => t!("tools.control"),
		WineTool::Explorer => t!("tools.explorer"),
		WineTool::Console => t!("tools.console"),
	}
	.into_owned()
}
