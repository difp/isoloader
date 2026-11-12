use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui::{self, Color32, CornerRadius, FontFamily, FontId, RichText, Stroke, Vec2};
use sha2::Digest;

// Anthropic Editorial Warm Palette
const BG_WARM: Color32 = Color32::from_rgb(250, 248, 245); // Sand / warm ivory background
const CARD_BG: Color32 = Color32::from_rgb(255, 255, 255); // Pure card white
const CARD_BORDER: Color32 = Color32::from_rgb(232, 226, 217); // Subtle warm outline
const TEXT_PRIMARY: Color32 = Color32::from_rgb(33, 31, 28); // Warm deep charcoal
const TEXT_MUTED: Color32 = Color32::from_rgb(117, 110, 100); // Warm earth grey
const ACCENT_TERRACOTTA: Color32 = Color32::from_rgb(190, 91, 57); // Claude terracotta accent
const ACCENT_TERRACOTTA_LIGHT: Color32 = Color32::from_rgb(247, 237, 231); // Warm clay highlight
const WARN_COLOR: Color32 = Color32::from_rgb(165, 87, 42); // Warm warning
const SUCCESS_COLOR: Color32 = Color32::from_rgb(60, 116, 75); // Natural forest green

#[derive(Clone, Debug)]
struct Drive {
    path: PathBuf,
    name: String,
    model: String,
    bytes: u64,
    removable: bool,
}

enum Message {
    Progress(u64, u64),
    Finished(Result<(), String>),
}

struct App {
    image: Option<PathBuf>,
    drives: Vec<Drive>,
    selected: Option<usize>,
    verify: bool,
    progress: f32,
    progress_text: String,
    status: String,
    busy: bool,
    rx: Option<Receiver<Message>>,
}

impl Default for App {
    fn default() -> Self {
        Self {
            image: None,
            drives: discover_drives(),
            selected: None,
            verify: true,
            progress: 0.0,
            progress_text: "Ready".into(),
            status: "Select a disk image and a destination USB drive to begin.".into(),
            busy: false,
            rx: None,
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive_messages(ui.ctx());

        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(20.0);

            // Top Header: Anthropic-style typography and calm brand icon
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("✦")
                        .font(FontId::new(20.0, FontFamily::Proportional))
                        .color(ACCENT_TERRACOTTA),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("ISOLoader")
                        .font(FontId::new(20.0, FontFamily::Proportional))
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(10.0);
                ui.label(
                    RichText::new("—  A thoughtful utility for bootable drives")
                        .font(FontId::new(14.0, FontFamily::Proportional))
                        .color(TEXT_MUTED),
                );
            });

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(22.0);

            // Editorial Headline
            ui.label(
                RichText::new("Prepare your USB drive")
                    .font(FontId::new(28.0, FontFamily::Proportional))
                    .strong()
                    .color(TEXT_PRIMARY),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Write an ISO or disk image directly to removable media with block-level safety.")
                    .font(FontId::new(14.5, FontFamily::Proportional))
                    .color(TEXT_MUTED),
            );
            ui.add_space(24.0);

            // Step 1: Image File
            card(ui, |ui| {
                section_header(ui, "01", "Disk Image");
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    let (name, size_str) = self
                        .image
                        .as_ref()
                        .map(|p| {
                            let n = p.file_name().unwrap_or_default().to_string_lossy().to_string();
                            let s = size(p.metadata().map(|m| m.len()).unwrap_or(0));
                            (n, s)
                        })
                        .unwrap_or_else(|| ("No disk image selected".into(), "".into()));

                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(name)
                                .font(FontId::new(15.0, FontFamily::Proportional))
                                .strong()
                                .color(if self.image.is_some() { TEXT_PRIMARY } else { TEXT_MUTED }),
                        );
                        if !size_str.is_empty() {
                            ui.label(
                                RichText::new(format!("File size: {size_str}"))
                                    .font(FontId::new(13.0, FontFamily::Proportional))
                                    .color(TEXT_MUTED),
                            );
                        }
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let btn = egui::Button::new(
                            RichText::new("Choose file…")
                                .font(FontId::new(14.0, FontFamily::Proportional))
                                .color(ACCENT_TERRACOTTA),
                        )
                        .fill(ACCENT_TERRACOTTA_LIGHT)
                        .stroke(Stroke::new(1.0, CARD_BORDER))
                        .corner_radius(CornerRadius::same(8));

                        if ui.add_enabled(!self.busy, btn).clicked() {
                            self.image = rfd::FileDialog::new()
                                .add_filter("Disk images", &["iso", "img", "dmg"])
                                .pick_file();
                        }
                    });
                });
            });

            ui.add_space(16.0);

            // Step 2: Destination Device
            card(ui, |ui| {
                ui.horizontal(|ui| {
                    section_header(ui, "02", "Destination Device");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let refresh_btn = egui::Button::new(
                            RichText::new("↻  Refresh list")
                                .font(FontId::new(13.0, FontFamily::Proportional))
                                .color(TEXT_MUTED),
                        )
                        .fill(Color32::TRANSPARENT)
                        .corner_radius(CornerRadius::same(6));

                        if ui.add_enabled(!self.busy, refresh_btn).clicked() {
                            self.drives = discover_drives();
                            self.selected = None;
                        }
                    });
                });

                ui.add_space(10.0);

                if self.drives.is_empty() {
                    ui.label(
                        RichText::new("No removable storage devices detected. Insert a USB flash drive and click Refresh.")
                            .font(FontId::new(13.5, FontFamily::Proportional))
                            .color(TEXT_MUTED),
                    );
                } else {
                    for (i, drive) in self.drives.iter().enumerate() {
                        let is_selected = self.selected == Some(i);
                        let bg = if is_selected {
                            ACCENT_TERRACOTTA_LIGHT
                        } else {
                            Color32::TRANSPARENT
                        };
                        let border_stroke = if is_selected {
                            Stroke::new(1.5, ACCENT_TERRACOTTA)
                        } else {
                            Stroke::new(1.0, CARD_BORDER)
                        };

                        let frame = egui::Frame::new()
                            .fill(bg)
                            .stroke(border_stroke)
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(egui::Margin::symmetric(14, 10));

                        let resp = frame
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    let icon = if drive.removable { "⬚" } else { "⬡" };
                                    ui.label(
                                        RichText::new(icon)
                                            .font(FontId::new(18.0, FontFamily::Proportional))
                                            .color(if is_selected { ACCENT_TERRACOTTA } else { TEXT_MUTED }),
                                    );
                                    ui.add_space(6.0);
                                    ui.vertical(|ui| {
                                        ui.label(
                                            RichText::new(&drive.model)
                                                .font(FontId::new(15.0, FontFamily::Proportional))
                                                .strong()
                                                .color(TEXT_PRIMARY),
                                        );
                                        ui.label(
                                            RichText::new(format!("{}  ·  {}", drive.path.display(), size(drive.bytes)))
                                                .font(FontId::new(12.5, FontFamily::Proportional))
                                                .color(TEXT_MUTED),
                                        );
                                    });
                                })
                            })
                            .response;

                        if resp.interact(egui::Sense::click()).clicked() && !self.busy {
                            self.selected = Some(i);
                        }
                        ui.add_space(6.0);
                    }
                }
            });

            ui.add_space(16.0);

            // Step 3: Options & Status
            card(ui, |ui| {
                section_header(ui, "03", "Verification & Progress");
                ui.add_space(6.0);

                ui.checkbox(
                    &mut self.verify,
                    RichText::new("Verify written data against SHA-256 hash")
                        .font(FontId::new(14.0, FontFamily::Proportional))
                        .color(TEXT_PRIMARY),
                );

                ui.add_space(12.0);

                let status_color = if self.busy {
                    ACCENT_TERRACOTTA
                } else if self.status.contains("successfully") {
                    SUCCESS_COLOR
                } else {
                    TEXT_MUTED
                };

                ui.label(
                    RichText::new(&self.status)
                        .font(FontId::new(13.5, FontFamily::Proportional))
                        .color(status_color),
                );

                ui.add_space(6.0);

                // Progress Bar
                let progress_bar = egui::ProgressBar::new(self.progress)
                    .text(
                        RichText::new(&self.progress_text)
                            .font(FontId::new(12.0, FontFamily::Proportional))
                            .color(TEXT_PRIMARY),
                    )
                    .corner_radius(CornerRadius::same(6))
                    .fill(ACCENT_TERRACOTTA);
                ui.add(progress_bar);

                ui.add_space(14.0);

                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("⚠  Writing will permanently overwrite all files on the selected drive.")
                            .font(FontId::new(12.5, FontFamily::Proportional))
                            .color(WARN_COLOR),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_flash = self.image.is_some() && self.selected.is_some() && !self.busy;
                        let flash_btn = egui::Button::new(
                            RichText::new(if self.busy { "Writing… " } else { "Write to USB  →" })
                                .font(FontId::new(15.0, FontFamily::Proportional))
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(if can_flash { ACCENT_TERRACOTTA } else { Color32::from_rgb(215, 208, 198) })
                        .corner_radius(CornerRadius::same(8))
                        .min_size(Vec2::new(140.0, 36.0));

                        if ui.add_enabled(can_flash, flash_btn).clicked() {
                            self.start_flash(ui.ctx());
                        }
                    });
                });
            });

            ui.add_space(24.0);
        });
    }
}

fn card(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(CARD_BG)
        .stroke(Stroke::new(1.0, CARD_BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(18.0)
        .show(ui, add);
}

fn section_header(ui: &mut egui::Ui, num: &str, title: &str) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(num)
                .font(FontId::new(13.0, FontFamily::Monospace))
                .color(ACCENT_TERRACOTTA),
        );
        ui.add_space(4.0);
        ui.label(
            RichText::new(title)
                .font(FontId::new(16.0, FontFamily::Proportional))
                .strong()
                .color(TEXT_PRIMARY),
        );
    });
}

fn size(bytes: u64) -> String {
    let mut n = bytes as f64;
    let units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut i = 0;
    while n >= 1024.0 && i < 4 {
        n /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{n:.1} {}", units[i])
    }
}

fn discover_drives() -> Vec<Drive> {
    let mut result = Vec::new();
    #[cfg(target_os = "linux")]
    if let Ok(entries) = std::fs::read_dir("/sys/block") {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with("loop")
                || name.starts_with("ram")
                || name.starts_with("dm-")
                || name.starts_with("sr")
            {
                continue;
            }
            let sys = entry.path();
            let sectors = std::fs::read_to_string(sys.join("size"))
                .unwrap_or_default()
                .trim()
                .parse::<u64>()
                .unwrap_or(0);
            let bytes = sectors.saturating_mul(512);
            if bytes == 0 {
                continue;
            }
            let removable = std::fs::read_to_string(sys.join("removable"))
                .unwrap_or_default()
                .trim()
                == "1";
            let model = std::fs::read_to_string(sys.join("device/model"))
                .unwrap_or_else(|_| name.clone())
                .trim()
                .to_owned();
            result.push(Drive {
                path: PathBuf::from(format!("/dev/{name}")),
                name,
                model,
                bytes,
                removable,
            });
        }
    }
    result.sort_by(|a, b| b.removable.cmp(&a.removable).then(a.name.cmp(&b.name)));
    result
}

impl App {
    fn receive_messages(&mut self, ctx: &egui::Context) {
        let mut done = None;
        if let Some(rx) = &self.rx {
            while let Ok(message) = rx.try_recv() {
                match message {
                    Message::Progress(done_bytes, total) => {
                        self.progress = if total == 0 {
                            0.0
                        } else {
                            done_bytes as f32 / total as f32
                        };
                        self.progress_text = format!("{} / {}", size(done_bytes), size(total));
                    }
                    Message::Finished(result) => done = Some(result),
                }
            }
        }
        if let Some(result) = done {
            let ok = result.is_ok();
            self.busy = false;
            self.rx = None;
            self.status = result
                .map(|_| "Image written and verified successfully.".into())
                .unwrap_or_else(|e| format!("Flash failed: {e}"));
            if ok {
                self.progress = 1.0;
                self.progress_text = "Complete".into();
            }
        }
        if self.busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn start_flash(&mut self, ctx: &egui::Context) {
        let image = self.image.clone().unwrap();
        let drive = self.drives[self.selected.unwrap()].clone();
        let verify = self.verify;
        if !drive.removable {
            self.status = "Selected device is not marked removable. Refusing to flash.".into();
            return;
        }
        self.busy = true;
        self.progress = 0.0;
        self.progress_text = "Starting…".into();
        self.status = "Flashing image. Keep the drive connected.".into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        thread::spawn(move || {
            let result = flash_image(&image, &drive, verify, &tx);
            let _ = tx.send(Message::Finished(result));
        });
        ctx.request_repaint();
    }
}

fn flash_image(
    image: &Path,
    target: &Drive,
    verify: bool,
    tx: &Sender<Message>,
) -> Result<(), String> {
    let mut src = File::open(image).map_err(|e| e.to_string())?;
    let total = src.metadata().map_err(|e| e.to_string())?.len();
    if total == 0 {
        return Err("image is empty".into());
    }
    if total > target.bytes {
        return Err("image is larger than the selected drive".into());
    }
    let mut dst = OpenOptions::new()
        .write(true)
        .open(&target.path)
        .map_err(|e| e.to_string())?;
    let mut hash = sha2::Sha256::new();
    let mut buf = vec![0u8; 4 * 1024 * 1024];
    let mut done = 0;
    loop {
        let n = src.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        dst.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        hash.update(&buf[..n]);
        done += n as u64;
        let _ = tx.send(Message::Progress(done, total));
    }
    dst.flush().map_err(|e| e.to_string())?;
    dst.sync_all().map_err(|e| e.to_string())?;
    if verify {
        drop(dst);
        let mut check = File::open(&target.path).map_err(|e| e.to_string())?;
        let mut remaining = total;
        let mut read_hash = sha2::Sha256::new();
        while remaining > 0 {
            let chunk = std::cmp::min(buf.len() as u64, remaining) as usize;
            let n = check.read(&mut buf[..chunk]).map_err(|e| e.to_string())?;
            if n == 0 {
                return Err("target ended before image length".into());
            }
            read_hash.update(&buf[..n]);
            remaining -= n as u64;
        }
        if hash.finalize() != read_hash.finalize() {
            return Err("SHA-256 verification failed".into());
        }
    }
    Ok(())
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(Vec2::new(860.0, 720.0))
            .with_min_inner_size(Vec2::new(700.0, 580.0)),
        ..Default::default()
    };
    eframe::run_native(
        "ISOLoader",
        options,
        Box::new(|cc| {
            // Apply Anthropic light theme visuals
            let mut visuals = egui::Visuals::light();
            visuals.panel_fill = BG_WARM;
            visuals.window_fill = BG_WARM;
            visuals.override_text_color = Some(TEXT_PRIMARY);
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::new(App::default()))
        }),
    )
}
