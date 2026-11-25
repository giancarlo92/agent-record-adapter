// Part of Agent Record Adapter (see NOTICE).
//! The bar and its panels, drawn to the measures of the design: a 230-point bar (grip, close,
//! settings, the round Record / Stop button, the recordings) and, 6 points below it, a 250-point
//! card with the settings rows or the recordings.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, Ui,
    ViewportCommand, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::app::{App, Panel, hide_from_capture};
use crate::look::{self, BAR_WIDTH, BIG, GAP, Icon, PANEL_WIDTH, Palette, ROW, SMALL};

const PAD: f32 = 10.0;
const TOP: f32 = 6.0;
const TOP_ROW: f32 = 16.0;
/// Room between the grip and close row and the buttons, as Windows voice typing leaves.
const TOP_GAP: f32 = 12.0;
const BOTTOM: f32 = 12.0;
const LABEL: f32 = 13.0;
const CAPTION: f32 = 12.0;
/// One step in a recording's step list.
const STEP_LINE: f32 = 18.0;

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        let held = std::mem::take(&mut self.held_input);
        let (events, later) = steady_clicks(held, std::mem::take(&mut raw_input.events));
        raw_input.events = events;
        if !later.is_empty() {
            self.held_input = later;
            ctx.request_repaint();
        }
    }

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll(ctx);
        let recording = self.active.is_some();
        if recording != self.hidden_from_capture {
            hide_from_capture(frame, recording);
            self.hidden_from_capture = recording;
        }
        let p = if ctx.style().visuals.dark_mode {
            look::DARK
        } else {
            look::LIGHT
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                let bar = self.bar(ui, ctx, &p);
                let mut bottom = bar.bottom();
                if !recording && self.panel != Panel::None {
                    let top = bar.bottom() + GAP;
                    bottom = match self.panel {
                        Panel::Settings => self.settings(ui, top, &p),
                        Panel::List => self.list(ui, top, &p),
                        Panel::None => bar.bottom(),
                    };
                }
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(vec2(
                    PANEL_WIDTH,
                    bottom.ceil() + 1.0,
                )));
            });
    }
}

impl App {
    /// The bar: grip and close on top, then settings, Record / Stop and the recordings.
    fn bar(&mut self, ui: &mut Ui, ctx: &egui::Context, p: &Palette) -> Rect {
        let recording = self.active.as_ref().map(|a| a.started.elapsed().as_secs());
        let row_height = if recording.is_some() {
            BIG + 4.0 + 17.0
        } else {
            BIG
        };
        let error_height = if self.error.is_some() { 18.0 } else { 0.0 };
        let bar = Rect::from_min_size(
            Pos2::ZERO,
            vec2(
                BAR_WIDTH,
                TOP + TOP_ROW + TOP_GAP + row_height + error_height + BOTTOM,
            ),
        );
        look::card(ui, bar, p);

        // The grip (drag to move) and the close button.
        let grip_area =
            Rect::from_min_size(pos2(PAD, TOP), vec2(BAR_WIDTH - 2.0 * PAD - 18.0, TOP_ROW));
        if ui
            .interact(grip_area, ui.id().with("grip"), Sense::drag())
            .drag_started()
        {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        let grip = Rect::from_center_size(pos2(BAR_WIDTH / 2.0, TOP + 7.0), vec2(34.0, 4.0));
        ui.painter()
            .rect_filled(grip, CornerRadius::same(2), p.muted);
        let close =
            Rect::from_center_size(pos2(BAR_WIDTH - PAD - 8.0, TOP + 6.0), vec2(18.0, 18.0));
        let response = button(ui, close, "close", self.t.close);
        Icon::Close.paint(
            ui,
            close,
            16.0,
            if response.hovered() {
                p.text
            } else {
                p.secondary
            },
        );
        if response.clicked() {
            if self.active.is_some() {
                // Saved first: the bar closes when agent-record has written the files.
                self.close_when_saved = true;
                self.stop();
            } else {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }

        // Settings, Record / Stop, the recordings.
        let row_top = TOP + TOP_ROW + TOP_GAP;
        let side_y = row_top + row_height / 2.0;
        let gear =
            Rect::from_center_size(pos2(PAD + 6.0 + SMALL / 2.0, side_y), vec2(SMALL, SMALL));
        if self.small(
            ui,
            gear,
            Icon::Settings,
            self.panel == Panel::Settings,
            recording.is_some(),
            self.t.settings,
            p,
        ) {
            self.panel = if self.panel == Panel::Settings {
                Panel::None
            } else {
                self.list_targets();
                Panel::Settings
            };
            self.choosing_target = false;
        }
        let list = Rect::from_center_size(
            pos2(BAR_WIDTH - PAD - 6.0 - SMALL / 2.0, side_y),
            vec2(SMALL, SMALL),
        );
        if self.small(
            ui,
            list,
            Icon::List,
            self.panel == Panel::List,
            recording.is_some(),
            self.t.recordings,
            p,
        ) {
            self.refresh();
            self.panel = if self.panel == Panel::List {
                Panel::None
            } else {
                Panel::List
            };
        }
        let big =
            Rect::from_center_size(pos2(BAR_WIDTH / 2.0, row_top + BIG / 2.0), vec2(BIG, BIG));
        let label = if recording.is_some() {
            self.t.stop
        } else {
            self.t.record
        };
        let response = button(ui, big, "record", label);
        let ring = if recording.is_some() {
            p.danger
        } else if response.hovered() {
            p.secondary
        } else {
            p.border
        };
        ui.painter().circle(
            big.center(),
            BIG / 2.0 - 0.5,
            p.raised,
            Stroke::new(1.0, ring),
        );
        if recording.is_some() {
            ui.painter().rect_filled(
                Rect::from_center_size(big.center(), vec2(20.0, 20.0)),
                CornerRadius::same(4),
                p.danger,
            );
        } else {
            ui.painter().circle_filled(big.center(), 11.0, p.danger);
        }
        let saving = self.active.as_ref().is_some_and(|a| a.stopping);
        if saving {
            ui.painter().text(
                pos2(BAR_WIDTH / 2.0, big.bottom() + 4.0 + 8.5),
                Align2::CENTER_CENTER,
                self.t.saving,
                FontId::proportional(CAPTION),
                p.secondary,
            );
        } else if let Some(seconds) = recording {
            let time = format!("{:02}:{:02}", seconds / 60, seconds % 60);
            ui.painter().text(
                pos2(BAR_WIDTH / 2.0, big.bottom() + 4.0 + 8.5),
                Align2::CENTER_CENTER,
                time,
                FontId::monospace(13.0),
                p.danger,
            );
        }
        if response.clicked() && !saving && !self.starting {
            if recording.is_some() {
                self.stop()
            } else {
                self.start()
            }
        }
        if let Some(error) = &self.error {
            let at = pos2(PAD, row_top + row_height + 2.0);
            let text = fit(ui, error, BAR_WIDTH - 2.0 * PAD, CAPTION);
            ui.painter().text(
                at,
                Align2::LEFT_TOP,
                text,
                FontId::proportional(CAPTION),
                p.danger,
            );
        }
        bar
    }

    /// One of the two small square buttons. Returns whether it was clicked.
    #[allow(clippy::too_many_arguments)]
    fn small(
        &self,
        ui: &mut Ui,
        rect: Rect,
        icon: Icon,
        on: bool,
        off: bool,
        label: &str,
        p: &Palette,
    ) -> bool {
        let response = ui.interact(rect, ui.id().with(label), Sense::click());
        response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, !off, label));
        let fill = if on {
            p.accent_bg
        } else if response.hovered() && !off {
            p.hover
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(8), fill);
        let color = if off {
            p.muted
        } else if on {
            p.accent
        } else {
            p.text
        };
        icon.paint(ui, rect, 20.0, color);
        // A newer version is out: a dot on the settings button, where the update is.
        if label == self.t.settings && self.newer.is_some() && !off {
            ui.painter().circle_filled(
                pos2(rect.right() - 7.0, rect.top() + 7.0),
                3.5,
                p.accent_fill,
            );
        }
        response.clicked() && !off
    }

    /// The settings card: what to record, with video, where to save.
    fn settings(&mut self, ui: &mut Ui, top: f32, p: &Palette) -> f32 {
        let targets = if self.choosing_target {
            self.targets.len() as f32 * 28.0 + 6.0
        } else {
            0.0
        };
        let rows = 4.0;
        let card = Rect::from_min_size(pos2(0.0, top), vec2(PANEL_WIDTH, ROW * rows + targets));
        look::card(ui, card, p);
        let mut y = top;

        let current = self
            .targets
            .get(self.target)
            .map(|t| short_target(&t.label))
            .unwrap_or_default();
        let row = Rect::from_min_size(pos2(0.0, y), vec2(PANEL_WIDTH, ROW));
        if self.row(ui, row, Icon::Desktop, self.t.what, &current, p) {
            // Opens at once with the list already known; a fresh one replaces it when it comes.
            self.choosing_target = !self.choosing_target;
            if self.choosing_target {
                self.list_targets();
            }
        }
        y += ROW;
        if self.choosing_target {
            let mut chosen = None;
            for (index, target) in self.targets.iter().enumerate() {
                let item = Rect::from_min_size(
                    pos2(PAD + 30.0, y + 3.0),
                    vec2(PANEL_WIDTH - 2.0 * PAD - 30.0, 26.0),
                );
                let response = ui.interact(item, ui.id().with(("target", index)), Sense::click());
                response.widget_info(|| {
                    WidgetInfo::selected(
                        WidgetType::RadioButton,
                        true,
                        index == self.target,
                        &target.label,
                    )
                });
                if index == self.target || response.hovered() {
                    ui.painter().rect_filled(
                        item,
                        CornerRadius::same(6),
                        if index == self.target {
                            p.accent_bg
                        } else {
                            p.hover
                        },
                    );
                }
                let color = if index == self.target {
                    p.accent
                } else {
                    p.text
                };
                let text = fit(ui, &target.label, item.width() - 12.0, CAPTION);
                ui.painter().text(
                    pos2(item.left() + 6.0, item.center().y),
                    Align2::LEFT_CENTER,
                    text,
                    FontId::proportional(CAPTION),
                    color,
                );
                if response.clicked() {
                    chosen = Some(index);
                }
                y += 28.0;
            }
            y += 6.0;
            if let Some(index) = chosen {
                self.target = index;
                self.choosing_target = false;
            }
        }
        hairline(ui, y, p);

        let row = Rect::from_min_size(pos2(0.0, y), vec2(PANEL_WIDTH, ROW));
        let response = ui.interact(row, ui.id().with("video"), Sense::click());
        response.widget_info(|| {
            WidgetInfo::selected(WidgetType::Checkbox, true, self.video, self.t.video)
        });
        Icon::Video.paint(
            ui,
            Rect::from_min_size(pos2(12.0, row.top()), vec2(18.0, ROW)),
            18.0,
            p.text,
        );
        ui.painter().text(
            pos2(40.0, row.center().y),
            Align2::LEFT_CENTER,
            self.t.video,
            FontId::proportional(LABEL),
            p.text,
        );
        look::switch(
            ui,
            Rect::from_center_size(
                pos2(PANEL_WIDTH - 12.0 - 17.0, row.center().y),
                vec2(34.0, 18.0),
            ),
            self.video,
            p,
        );
        if response.clicked() {
            self.video = !self.video;
            crate::tool::save_settings(&self.folder, self.video);
        }
        y += ROW;
        hairline(ui, y, p);

        let folder = self
            .folder
            .rsplit(['\\', '/'])
            .find(|s| !s.is_empty())
            .unwrap_or(&self.folder)
            .to_owned();
        let row = Rect::from_min_size(pos2(0.0, y), vec2(PANEL_WIDTH, ROW));
        if self.row(ui, row, Icon::Folder, self.t.folder, &folder, p) {
            self.choose_folder();
        }
        y += ROW;
        // The version, always: with a newer one out, install it and open the app again, as
        // Claude Code and Codex do; else a click asks npm again.
        hairline(ui, y, p);
        let row = Rect::from_min_size(pos2(0.0, y), vec2(PANEL_WIDTH, ROW));
        if let Some(version) = self.newer.clone() {
            let value = if self.updating {
                self.t.updating.to_owned()
            } else {
                format!("{} → {version}", self.version)
            };
            if self.row(ui, row, Icon::Download, self.t.update, &value, p) {
                self.update();
            }
        } else {
            let value = if self.checking {
                self.t.checking.to_owned()
            } else if self.offline {
                format!("{} · {}", self.version, self.t.offline)
            } else {
                format!("{} · {}", self.version, self.t.up_to_date)
            };
            if self.row(ui, row, Icon::Refresh, self.t.version, &value, p) {
                self.check_now();
            }
        }
        y += ROW;
        y
    }

    /// A settings row: icon, label, the value on the right and an arrow. Returns whether it was clicked.
    fn row(
        &self,
        ui: &mut Ui,
        row: Rect,
        icon: Icon,
        label: &str,
        value: &str,
        p: &Palette,
    ) -> bool {
        let response = ui.interact(row.shrink(1.0), ui.id().with(label), Sense::click());
        response.widget_info(|| {
            WidgetInfo::labeled(WidgetType::Button, true, format!("{label}: {value}"))
        });
        icon.paint(
            ui,
            Rect::from_min_size(pos2(12.0, row.top()), vec2(18.0, ROW)),
            18.0,
            p.text,
        );
        ui.painter().text(
            pos2(40.0, row.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(LABEL),
            p.text,
        );
        let chevron = Rect::from_center_size(
            pos2(PANEL_WIDTH - 12.0 - 6.5, row.center().y),
            vec2(13.0, 13.0),
        );
        Icon::Chevron.paint(ui, chevron, 13.0, p.text);
        let label_end = 40.0 + text_width(ui, label, LABEL) + 8.0;
        let value = fit(ui, value, chevron.left() - 10.0 - label_end, LABEL);
        ui.painter().text(
            pos2(chevron.left() - 10.0, row.center().y),
            Align2::RIGHT_CENTER,
            value,
            FontId::proportional(LABEL),
            p.secondary,
        );
        response.clicked()
    }

    /// The recordings card: each with Play, Repair (a prompt for the person's AI agent) and Open
    /// folder.
    fn list(&mut self, ui: &mut Ui, top: f32, p: &Palette) -> f32 {
        let mut heights = Vec::new();
        for item in &self.items {
            heights.push(ROW + self.status_height(ui, item, p) + self.steps_height(item));
        }
        let empty = if self.items.is_empty() { ROW } else { 0.0 };
        let card = Rect::from_min_size(
            pos2(0.0, top),
            vec2(PANEL_WIDTH, heights.iter().sum::<f32>() + empty),
        );
        look::card(ui, card, p);
        let mut y = top;
        if self.items.is_empty() {
            ui.painter().text(
                pos2(12.0, y + ROW / 2.0),
                Align2::LEFT_CENTER,
                self.t.none_yet,
                FontId::proportional(LABEL),
                p.secondary,
            );
            y += ROW;
            hairline(ui, y, p);
        }
        let mut play = None;
        let mut cancel = false;
        let mut open_steps: Option<std::path::PathBuf> = None;
        let mut toggle: Option<(std::path::PathBuf, usize)> = None;
        let mut repair: Option<(std::path::PathBuf, bool)> = None;
        let mut hovered_repair = None;
        let count = self.items.len();
        for (index, (item, height)) in self.items.iter().zip(heights).enumerate() {
            let line = pos2(12.0, y + ROW / 2.0);
            let label = format!(
                "{} · {} {}",
                item.when,
                item.steps,
                if item.steps == 1 {
                    self.t.step
                } else {
                    self.t.steps
                }
            );
            ui.painter().text(
                line,
                Align2::LEFT_CENTER,
                label,
                FontId::proportional(LABEL),
                p.text,
            );
            let folder =
                Rect::from_center_size(pos2(PANEL_WIDTH - 12.0 - 9.0, line.y), vec2(24.0, 24.0));
            let repair_at = Rect::from_center_size(
                pos2(folder.center().x - 9.0 - 10.0 - 9.0, line.y),
                vec2(24.0, 24.0),
            );
            let play_at = Rect::from_center_size(
                pos2(repair_at.center().x - 9.0 - 10.0 - 9.0, line.y),
                vec2(24.0, 24.0),
            );
            // The recording's line opens (and closes) the list of its steps.
            let label_at = Rect::from_min_max(
                pos2(8.0, y + 4.0),
                pos2(play_at.left() - 8.0, y + ROW - 4.0),
            );
            if item.steps > 0
                && button(
                    ui,
                    label_at,
                    &format!("steps{}", item.dir.display()),
                    &format!("{}: {}", self.t.show_steps, folder_of(item)),
                )
                .clicked()
            {
                open_steps = Some(item.dir.clone());
            }
            let can_play = self.playing.is_none() && item.steps > 0;
            let response = button(
                ui,
                play_at,
                &format!("play{}", item.dir.display()),
                &format!("{}: {}", self.t.play, folder_of(item)),
            );
            Icon::Play.paint(
                ui,
                play_at,
                18.0,
                if !can_play {
                    p.muted
                } else if response.hovered() {
                    p.accent
                } else {
                    p.text
                },
            );
            if response.clicked() && can_play {
                play = Some(item.dir.clone());
            }
            // Repair: one click copies a prompt for the person's AI agent; a double click also
            // opens Claude with it written. It stands out when the last play failed.
            let response = button(
                ui,
                repair_at,
                &format!("repair{}", item.dir.display()),
                &format!("{}: {}", self.t.repair, folder_of(item)),
            );
            let failed = item.failed.is_some()
                || self
                    .results
                    .iter()
                    .any(|(dir, result)| *dir == item.dir && result.starts_with(self.t.failed));
            Icon::Repair.paint(
                ui,
                repair_at,
                17.0,
                if response.hovered() || failed {
                    p.accent
                } else {
                    p.text
                },
            );
            if response.hovered() {
                hovered_repair = Some(item.dir.clone());
            }
            if response.double_clicked() {
                repair = Some((item.dir.clone(), true));
            } else if response.clicked() {
                repair = Some((item.dir.clone(), false));
            }
            let response = button(
                ui,
                folder,
                &format!("folder{}", item.dir.display()),
                &format!("{}: {}", self.t.open_folder, folder_of(item)),
            );
            Icon::Folder.paint(
                ui,
                folder,
                18.0,
                if response.hovered() { p.accent } else { p.text },
            );
            if response.clicked() {
                crate::tool::open_folder(&item.dir);
            }
            if let Some(lines) = self.status(item) {
                let mut at = y + ROW - 6.0;
                for (text, color) in lines {
                    let color = color.unwrap_or(p.secondary);
                    let galley = status_line(ui, &text, color);
                    let height = galley.size().y + 1.0;
                    ui.painter().galley(pos2(12.0, at), galley, color);
                    at += height;
                }
                if self.confirm.as_ref() == Some(&item.dir) {
                    let run = Rect::from_min_size(
                        pos2(12.0, at - 17.0),
                        vec2(text_width(ui, self.t.run, CAPTION) + 4.0, 16.0),
                    );
                    let no = Rect::from_min_size(
                        pos2(run.right() + 14.0, at - 17.0),
                        vec2(text_width(ui, self.t.cancel, CAPTION) + 4.0, 16.0),
                    );
                    if button(ui, run, &format!("run{}", item.dir.display()), self.t.run).clicked()
                    {
                        play = Some(item.dir.clone());
                    }
                    if button(
                        ui,
                        no,
                        &format!("cancel{}", item.dir.display()),
                        self.t.cancel,
                    )
                    .clicked()
                    {
                        cancel = true;
                    }
                }
            }
            if self.steps_open.as_ref() == Some(&item.dir) {
                let mut at = y + ROW + self.status_height(ui, item, p);
                ui.painter().text(
                    pos2(12.0, at + STEP_LINE / 2.0),
                    Align2::LEFT_CENTER,
                    self.t.mark_hint,
                    FontId::proportional(CAPTION),
                    p.secondary,
                );
                at += STEP_LINE;
                for (index, text, ask) in &item.lines {
                    let line =
                        Rect::from_min_size(pos2(8.0, at), vec2(PANEL_WIDTH - 16.0, STEP_LINE));
                    let text = fit(ui, text, PANEL_WIDTH - 44.0, CAPTION);
                    if let Some(on) = ask {
                        // A click: a box that says whether it asks before it runs.
                        // A checkbox for screen readers and agents too, ticked when it asks.
                        let response = ui.interact(
                            line,
                            ui.id().with(format!("ask{}#{index}", item.dir.display())),
                            Sense::click(),
                        );
                        response.widget_info(|| {
                            WidgetInfo::selected(WidgetType::Checkbox, true, *on, &text)
                        });
                        let mark =
                            Rect::from_center_size(pos2(19.0, line.center().y), vec2(11.0, 11.0));
                        let color = if response.hovered() { p.accent } else { p.text };
                        if *on {
                            ui.painter().rect_filled(mark, 2.0, p.accent_fill);
                            ui.painter().line_segment(
                                [
                                    pos2(mark.left() + 2.5, mark.center().y),
                                    pos2(mark.center().x - 0.5, mark.bottom() - 3.0),
                                ],
                                Stroke::new(1.5, p.on_accent),
                            );
                            ui.painter().line_segment(
                                [
                                    pos2(mark.center().x - 0.5, mark.bottom() - 3.0),
                                    pos2(mark.right() - 2.5, mark.top() + 2.5),
                                ],
                                Stroke::new(1.5, p.on_accent),
                            );
                        } else {
                            ui.painter().rect_stroke(
                                mark,
                                2.0,
                                Stroke::new(1.0, color),
                                egui::StrokeKind::Inside,
                            );
                        }
                        ui.painter().text(
                            pos2(32.0, line.center().y),
                            Align2::LEFT_CENTER,
                            text,
                            FontId::proportional(CAPTION),
                            color,
                        );
                        if response.clicked() {
                            toggle = Some((item.dir.clone(), *index));
                        }
                    } else {
                        ui.painter().text(
                            pos2(32.0, line.center().y),
                            Align2::LEFT_CENTER,
                            text,
                            FontId::proportional(CAPTION),
                            p.secondary,
                        );
                    }
                    at += STEP_LINE;
                }
            }
            y += height;
            if index + 1 < count {
                hairline(ui, y, p);
            }
        }
        if let Some(dir) = open_steps {
            self.steps_open = if self.steps_open.as_ref() == Some(&dir) {
                None
            } else {
                Some(dir)
            };
            ui.ctx().request_repaint();
        }
        if let Some((dir, step)) = toggle {
            self.toggle_ask_before(&dir, step);
            ui.ctx().request_repaint();
        }
        if hovered_repair != self.hovered_repair {
            self.hovered_repair = hovered_repair;
            ui.ctx().request_repaint();
        }
        if let Some((dir, open)) = repair {
            let prompt = crate::repair::prompt(&dir, self.t.spanish);
            ui.ctx().copy_text(prompt.clone());
            if open {
                crate::repair::open_in_claude(&dir, &prompt, self.t.spanish);
            }
            self.copied = Some((dir, std::time::Instant::now()));
        }
        if cancel {
            self.confirm = None;
        }
        if let Some(dir) = play {
            self.ask_to_play(&dir);
        }
        y
    }

    /// The height of the lines under a recording (see [`Self::status`]).
    fn status_height(&self, ui: &Ui, item: &crate::app::Item, p: &Palette) -> f32 {
        self.status(item).map_or(0.0, |lines| {
            lines
                .iter()
                .map(|(text, _)| status_line(ui, text, p.secondary).size().y + 1.0)
                .sum()
        })
    }

    /// The height of a recording's step list, when it is open: a hint and one line per step.
    fn steps_height(&self, item: &crate::app::Item) -> f32 {
        if self.steps_open.as_ref() == Some(&item.dir) {
            STEP_LINE * (item.lines.len() as f32 + 1.0) + 6.0
        } else {
            0.0
        }
    }

    /// The lines under a recording: playing, how it ended, or the steps to confirm.
    fn status(&self, item: &crate::app::Item) -> Option<Vec<(String, Option<Color32>)>> {
        if self
            .playing
            .as_ref()
            .is_some_and(|playing| playing.dir == item.dir)
        {
            return Some(vec![(
                self.playing
                    .as_ref()
                    .map(|p| p.last.clone())
                    .unwrap_or_default(),
                None,
            )]);
        }
        if self
            .copied
            .as_ref()
            .is_some_and(|(dir, _)| *dir == item.dir)
        {
            return Some(vec![(self.t.copied.to_owned(), None)]);
        }
        if self.hovered_repair.as_ref() == Some(&item.dir) && self.confirm.is_none() {
            return Some(vec![(self.t.repair_hint.to_owned(), None)]);
        }
        if self.confirm.as_ref() == Some(&item.dir) {
            let mut lines = vec![(self.t.irreversible.to_owned(), None)];
            lines.extend(item.irreversible.iter().map(|s| (format!("• {s}"), None)));
            lines.push((String::new(), None));
            return Some(lines);
        }
        if let Some((_, result)) = self.results.iter().find(|(dir, _)| *dir == item.dir) {
            return Some(vec![(result.clone(), None)]);
        }
        if let Some(failed) = &item.failed {
            return Some(vec![(failed.clone(), None)]);
        }
        if item.steps == 0 {
            return Some(vec![(self.t.nothing_recorded.to_owned(), None)]);
        }
        None
    }
}

/// Input that jumps onto the bar and clicks at once (an accessibility or
/// automation tool, a touch screen) reaches egui as a press and a release in
/// one frame with no hover before them, and egui drops that click. Such a
/// press gets a move to its spot first, and its release waits for the next
/// frame, as a mouse moved by hand delivers them. Returns this frame's events
/// and the ones held for the next.
pub(crate) fn steady_clicks(
    held: Vec<egui::Event>,
    events: Vec<egui::Event>,
) -> (Vec<egui::Event>, Vec<egui::Event>) {
    use egui::Event;
    let mut now = Vec::with_capacity(held.len() + events.len() + 1);
    let mut later = Vec::new();
    let mut hovered = false;
    let mut pressed = false;
    for event in held.into_iter().chain(events) {
        if !later.is_empty() {
            later.push(event);
            continue;
        }
        match &event {
            Event::PointerMoved(_) => hovered = true,
            Event::PointerGone => hovered = false,
            Event::PointerButton {
                pos, pressed: true, ..
            } => {
                if !hovered {
                    now.push(Event::PointerMoved(*pos));
                    hovered = true;
                }
                pressed = true;
            }
            Event::PointerButton { pressed: false, .. } if pressed => {
                later.push(event);
                continue;
            }
            _ => {}
        }
        now.push(event);
    }
    (now, later)
}

/// The recording's folder name ("Recording 2026-11-25 18.17.28"): it tells apart the buttons of
/// each recording for a screen reader or an agent.
fn folder_of(item: &crate::app::Item) -> String {
    item.dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| item.when.clone())
}

/// A clickable area with a name for screen readers and agents.
fn button(ui: &mut Ui, rect: Rect, id: &str, label: &str) -> Response {
    let response = ui.interact(rect, ui.id().with(id), Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    response
}

fn hairline(ui: &Ui, y: f32, p: &Palette) {
    ui.painter().line_segment(
        [pos2(1.0, y), pos2(PANEL_WIDTH - 1.0, y)],
        Stroke::new(1.0, p.hairline),
    );
}

fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(size), Color32::WHITE)
        .size()
        .x
}

/// A line under a recording, wrapped to the card's width (three rows at most, so a long error
/// does not stretch the card; the rest ends in "…").
fn status_line(ui: &Ui, text: &str, color: Color32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple(
        text.to_owned(),
        FontId::proportional(CAPTION),
        color,
        PANEL_WIDTH - 24.0,
    );
    job.wrap.max_rows = 3;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

/// The text, cut with "…" to fit `width`.
fn fit(ui: &Ui, text: &str, width: f32, size: f32) -> String {
    if text_width(ui, text, size) <= width {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty()
        && text_width(ui, &format!("{}…", chars.iter().collect::<String>()), size) > width
    {
        chars.pop();
    }
    format!("{}…", chars.into_iter().collect::<String>().trim_end())
}

/// "Pantalla 1" stays; a window "Report.xlsx - Excel  (EXCEL.EXE)" shows as "Report.xlsx - Excel".
fn short_target(label: &str) -> String {
    label
        .split("  (")
        .next()
        .unwrap_or(label)
        .trim_end_matches(" *")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::steady_clicks;
    use eframe::egui::{Event, Modifiers, PointerButton, pos2};

    fn button(pressed: bool) -> Event {
        Event::PointerButton {
            pos: pos2(40.0, 60.0),
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn a_jump_and_click_is_spread_over_two_frames() {
        let (now, later) = steady_clicks(Vec::new(), vec![button(true), button(false)]);
        assert_eq!(
            now,
            vec![Event::PointerMoved(pos2(40.0, 60.0)), button(true)]
        );
        assert_eq!(later, vec![button(false)]);
        let (next, rest) = steady_clicks(later, Vec::new());
        assert_eq!(next, vec![button(false)]);
        assert!(rest.is_empty());
    }

    #[test]
    fn a_hand_click_passes_unchanged() {
        let moved = Event::PointerMoved(pos2(40.0, 60.0));
        let (now, later) = steady_clicks(Vec::new(), vec![moved.clone(), button(true)]);
        assert_eq!(now, vec![moved, button(true)]);
        assert!(later.is_empty());
        let (now, later) = steady_clicks(Vec::new(), vec![button(false)]);
        assert_eq!(now, vec![button(false)]);
        assert!(later.is_empty());
    }
}
