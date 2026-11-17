#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, TryRecvError};

use eframe::egui::{self, Align, Color32, Frame, Layout, RichText, Stroke, Vec2};
use unique_uid::{CopyBatch, Error, SavedCopy, create_copies, read_uid};

const WIDTH: f32 = 410.0;
const BASE_HEIGHT: f32 = 193.0;
const WINDOW: Color32 = Color32::from_rgb(25, 28, 35);
const SURFACE: Color32 = Color32::from_rgb(35, 39, 48);
const TEXT: Color32 = Color32::from_rgb(234, 238, 247);
const MUTED: Color32 = Color32::from_rgb(174, 183, 202);
const BLUE: Color32 = Color32::from_rgb(74, 112, 186);
const ACTION_DISABLED: Color32 = Color32::from_rgb(52, 62, 82);
const PALE_BLUE: Color32 = Color32::from_rgb(172, 197, 248);
const OUTLINE: Color32 = Color32::from_rgb(64, 72, 91);
const WARNING: Color32 = Color32::from_rgb(255, 209, 128);

fn main() -> eframe::Result {
    let mut viewport = egui::ViewportBuilder::default()
        .with_app_id("unique-uid")
        .with_inner_size([WIDTH, BASE_HEIGHT])
        .with_resizable(false)
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    let (options, supports_file_drop) = {
        use winit::platform::x11::EventLoopBuilderExtX11;

        let mut options = options;
        let has_x11 = std::env::var_os("DISPLAY").is_some_and(|display| !display.is_empty());
        if has_x11 {
            // Winit 0.30 only implements incoming Linux file drops in its X11 backend.
            options.event_loop_builder = Some(Box::new(|builder| {
                builder.with_x11();
            }));
        }
        (options, has_x11)
    };
    #[cfg(not(target_os = "linux"))]
    let supports_file_drop = true;
    eframe::run_native(
        "Unique UID",
        options,
        Box::new(|cc| {
            let mut visuals = egui::Visuals::dark();
            visuals.override_text_color = Some(TEXT);
            visuals.selection.bg_fill = BLUE;
            visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, PALE_BLUE);
            cc.egui_ctx.set_visuals(visuals);
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "Inter".into(),
                egui::FontData::from_static(include_bytes!("../assets/Inter.ttf")).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "Inter".into());
            cc.egui_ctx.set_fonts(fonts);
            Ok(Box::new(UniqueUid {
                supports_file_drop,
                ..Default::default()
            }))
        }),
    )
}

enum Completed {
    File(PathBuf, Result<u64, Error>),
    Copies(Result<CopyBatch, Error>),
}

struct UniqueUid {
    source: Option<(PathBuf, u64)>,
    destination: Option<PathBuf>,
    count: String,
    issued_uids: HashSet<u64>,
    saved: Vec<SavedCopy>,
    error: Option<String>,
    work: Option<(&'static str, Receiver<Completed>)>,
    show_saved: bool,
    height: f32,
    supports_file_drop: bool,
}

impl Default for UniqueUid {
    fn default() -> Self {
        Self {
            source: None,
            destination: None,
            count: "1".into(),
            issued_uids: HashSet::new(),
            saved: Vec::new(),
            error: None,
            work: None,
            show_saved: false,
            height: BASE_HEIGHT,
            supports_file_drop: true,
        }
    }
}

impl UniqueUid {
    fn count(&self) -> Option<usize> {
        self.count.trim().parse().ok().filter(|&count| count > 0)
    }

    fn output_folder(&self) -> Option<&Path> {
        self.destination
            .as_deref()
            .or_else(|| self.source.as_ref().and_then(|(path, _)| path.parent()))
    }

    fn start(
        &mut self,
        ctx: &egui::Context,
        message: &'static str,
        task: impl FnOnce() -> Completed + Send + 'static,
    ) {
        self.error = None;
        self.saved.clear();
        let ctx = ctx.clone();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let _ = sender.send(task());
            ctx.request_repaint();
        });
        self.work = Some((message, receiver));
    }

    fn load_file(&mut self, ctx: &egui::Context, path: PathBuf) {
        self.start(ctx, "Reading file…", move || {
            let uid = read_uid(&path);
            Completed::File(path, uid)
        });
    }

    fn copy(&mut self, ctx: &egui::Context) {
        let (Some((source, _)), Some(destination), Some(count)) =
            (&self.source, self.output_folder(), self.count())
        else {
            return;
        };
        let source = source.clone();
        let destination = destination.to_owned();
        let excluded = self.issued_uids.clone();
        self.start(ctx, "Creating copies…", move || {
            Completed::Copies(create_copies(&source, &destination, count, excluded))
        });
    }

    fn poll_work(&mut self) {
        let Some((_, result)) = &self.work else {
            return;
        };
        let message = match result.try_recv() {
            Ok(message) => message,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => {
                self.error = Some("The operation ended unexpectedly.".into());
                self.work = None;
                return;
            }
        };
        self.work = None;
        match message {
            Completed::File(path, Ok(uid)) => self.source = Some((path, uid)),
            Completed::Copies(Ok(batch)) => {
                self.issued_uids
                    .extend(batch.copies.iter().map(|copy| copy.uid));
                self.saved = batch.copies;
                self.error = batch.error.map(|error| error.to_string());
            }
            Completed::File(_, Err(error)) | Completed::Copies(Err(error)) => {
                self.error = Some(error.to_string());
            }
        }
    }

    fn drop_tile(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame, hovering_file: bool) {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(ui.available_width(), 55.0), egui::Sense::click());
        let enabled = ui.is_enabled();
        let browse = if self.source.is_some() {
            "Change…"
        } else {
            "Browse…"
        };
        response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, browse));
        let selected = hovering_file || response.hovered() || response.has_focus();
        let painter = ui.painter_at(rect);
        painter.rect(
            rect,
            9,
            if selected {
                Color32::from_rgb(44, 58, 84)
            } else {
                SURFACE
            },
            Stroke::new(1.0, if selected { PALE_BLUE } else { OUTLINE }),
            egui::StrokeKind::Inside,
        );

        let badge = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 26.0, rect.center().y),
            egui::vec2(32.0, 32.0),
        );
        painter.rect_filled(badge, 7, Color32::from_rgb(47, 62, 92));
        let origin = badge.center();
        let point = |x, y| origin + egui::vec2(x, y);
        let stroke = Stroke::new(1.5, PALE_BLUE);
        painter.add(egui::Shape::line(
            vec![
                point(-6.0, 8.0),
                point(-6.0, -8.0),
                point(2.0, -8.0),
                point(6.0, -4.0),
                point(6.0, 8.0),
                point(-6.0, 8.0),
            ],
            stroke,
        ));
        painter.add(egui::Shape::line(
            vec![point(2.0, -8.0), point(2.0, -4.0), point(6.0, -4.0)],
            stroke,
        ));
        painter.line_segment([point(0.0, -2.0), point(0.0, 4.0)], stroke);
        painter.add(egui::Shape::line(
            vec![point(-2.5, 1.5), point(0.0, 4.0), point(2.5, 1.5)],
            stroke,
        ));

        let left = rect.left() + 51.0;
        if let Some((path, uid)) = &self.source {
            let name_rect = egui::Rect::from_min_max(
                egui::pos2(left, rect.top() + 9.0),
                egui::pos2(rect.right() - 75.0, rect.top() + 26.0),
            );
            let mut text = egui::text::LayoutJob::simple_singleline(
                file_name(path),
                egui::FontId::proportional(14.0),
                TEXT,
            );
            text.wrap.max_width = name_rect.width();
            text.wrap.max_rows = 1;
            text.wrap.break_anywhere = true;
            painter.galley(
                name_rect.min,
                ui.fonts_mut(|font| font.layout_job(text)),
                TEXT,
            );
            painter.text(
                egui::pos2(left, rect.top() + 30.0),
                egui::Align2::LEFT_TOP,
                format!("{uid:016X}"),
                egui::FontId::monospace(11.0),
                MUTED,
            );
        } else {
            painter.text(
                egui::pos2(left, rect.center().y),
                egui::Align2::LEFT_CENTER,
                if self.supports_file_drop {
                    "Drop an Avid .avb or .avc file"
                } else {
                    "Open an Avid .avb or .avc file"
                },
                egui::FontId::proportional(14.0),
                TEXT,
            );
        }
        painter.text(
            egui::pos2(rect.right() - 12.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            browse,
            egui::FontId::proportional(12.0),
            PALE_BLUE,
        );

        if response.clicked() {
            let mut picker = rfd::FileDialog::new()
                .set_parent(frame)
                .add_filter("Avid bins and scripts", &["avb", "avc", "AVB", "AVC"]);
            if let Some((file, _)) = &self.source
                && let Some(parent) = file.parent()
            {
                picker = picker.set_directory(parent);
            }
            if let Some(path) = picker.pick_file() {
                self.load_file(ui.ctx(), path);
            }
        }
    }

    fn settings(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, OUTLINE))
            .corner_radius(9)
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Copies");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let count = self.count().unwrap_or(1);
                        if ui.button("+").on_hover_text("Add a copy").clicked() {
                            self.count = count.saturating_add(1).to_string();
                        }
                        if ui
                            .add_enabled(count > 1, egui::Button::new("−"))
                            .on_hover_text("Remove a copy")
                            .clicked()
                        {
                            self.count = (count - 1).to_string();
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.count)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(41.0)
                                .horizontal_align(Align::Center),
                        )
                        .on_hover_text("Number of copies");
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("Save to");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .button("Choose…")
                            .on_hover_text("Choose output folder")
                            .clicked()
                        {
                            let mut picker = rfd::FileDialog::new().set_parent(frame);
                            if let Some(folder) = self.output_folder() {
                                picker = picker.set_directory(folder);
                            }
                            if let Some(folder) = picker.pick_folder() {
                                self.destination = Some(folder);
                            }
                        }
                        if self.destination.is_some()
                            && ui.button("↶").on_hover_text("Use source folder").clicked()
                        {
                            self.destination = None;
                        }
                        let label = self
                            .destination
                            .as_deref()
                            .map(file_name)
                            .unwrap_or_else(|| "Same as source".into());
                        ui.add(egui::Label::new(RichText::new(label).color(MUTED)).truncate())
                            .on_hover_text(
                                self.output_folder()
                                    .map(|folder| folder.display().to_string())
                                    .unwrap_or_else(|| "Save beside the source file".into()),
                            );
                    });
                });
            });
    }

    fn feedback(&mut self, ui: &mut egui::Ui) {
        if let Some((message, _)) = &self.work {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(*message).color(MUTED));
            });
        } else if !self.saved.is_empty() {
            ui.horizontal(|ui| {
                let count = self.saved.len();
                let label = if count == 1 {
                    "1 copy saved".into()
                } else {
                    format!("{count} copies saved")
                };
                ui.label(RichText::new(label).color(PALE_BLUE));
                if ui.button("View UIDs").clicked() {
                    self.show_saved = true;
                }
            });
        } else if self.count().is_none() {
            ui.label(RichText::new("Enter a positive whole number.").color(MUTED));
        }
        if let Some(message) = &self.error {
            ui.add(egui::Label::new(RichText::new(message).color(WARNING)).truncate())
                .on_hover_text(message);
        }
    }

    fn saved_view(&mut self, ui: &mut egui::Ui) {
        ui.heading("Saved Copies");
        ui.add_space(7.0);
        egui::ScrollArea::vertical()
            .max_height(ui.available_height() - 35.0)
            .show(ui, |ui| {
                for copy in &self.saved {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("{:016X}", copy.uid));
                        ui.add(egui::Label::new(file_name(&copy.path)).truncate())
                            .on_hover_text(copy.path.display().to_string());
                    });
                }
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Back").clicked() {
                self.show_saved = false;
            }
            if let Some(folder) = self.saved.first().and_then(|copy| copy.path.parent())
                && ui.button("Open Folder").clicked()
                && let Err(error) = open::that_detached(folder)
            {
                self.error = Some(format!("Could not open the output folder: {error}"));
                self.show_saved = false;
            }
        });
    }
}

impl eframe::App for UniqueUid {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.poll_work();
        let (hovering, dropped) = ctx.input(|input| {
            (
                !input.raw.hovered_files.is_empty(),
                input.raw.dropped_files.clone(),
            )
        });
        if self.work.is_none() && !dropped.is_empty() {
            match dropped.as_slice() {
                [file] if file.path.is_some() => {
                    self.show_saved = false;
                    self.load_file(ctx, file.path.clone().expect("file path"));
                }
                _ => self.error = Some("Drop one bin or script at a time.".into()),
            }
        }

        let has_status = self.work.is_some() || !self.saved.is_empty() || self.count().is_none();
        let height = if self.show_saved {
            252.0
        } else {
            BASE_HEIGHT
                + if has_status { 25.0 } else { 0.0 }
                + if self.error.is_some() { 22.0 } else { 0.0 }
        };
        if height != self.height {
            self.height = height;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(WIDTH, height)));
        }

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(WINDOW)
                    .inner_margin(egui::Margin::symmetric(13, 10)),
            )
            .show(ctx, |ui| {
                if self.show_saved {
                    self.saved_view(ui);
                    return;
                }
                ui.add_enabled_ui(self.work.is_none(), |ui| {
                    self.drop_tile(ui, frame, hovering);
                    ui.add_space(5.0);
                    self.settings(ui, frame);
                });
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    let enabled = self.work.is_none()
                        && self.source.is_some()
                        && self.output_folder().is_some()
                        && self.count().is_some();
                    let label = if self.count() == Some(1) {
                        "Create Copy"
                    } else {
                        "Create Copies"
                    };
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(RichText::new(label).size(14.0).strong())
                                .fill(if enabled { BLUE } else { ACTION_DISABLED })
                                .stroke(Stroke::new(1.0, if enabled { PALE_BLUE } else { MUTED }))
                                .corner_radius(8)
                                .min_size(Vec2::new(160.0, 32.0)),
                        )
                        .clicked()
                    {
                        self.copy(ctx);
                    }
                    self.feedback(ui);
                });
            });
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
