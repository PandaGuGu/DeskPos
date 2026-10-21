//! DeskPos GUI（egui 版）—— 布局列表 + 真实缩略预览
//!
//! 预览和 C#/WPF 版一样：用当前壁纸做背景、逐个图标取系统真实图标、
//! 按保存时的坐标摆好，连任务栏都画上。

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use deskpos::gui_icon::{self, IconProvider};
use deskpos::{actions, model::IconPos, model::Layout, repair, store};
use eframe::egui;
use egui::{Align, Align2, Color32, CornerRadius, FontId, Layout as EguiLayout, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Vec2};
use std::collections::HashMap;
use std::time::Duration;

// ---------------------------------------------------------------- 配色（与 WPF 版一致）
const BG: Color32 = Color32::from_rgb(0x0E, 0x11, 0x18);
const CARD: Color32 = Color32::from_rgb(0x17, 0x1B, 0x23);
const CARD_SEL: Color32 = Color32::from_rgb(0x1B, 0x22, 0x36);
const TEXT: Color32 = Color32::from_rgb(0xE6, 0xEA, 0xF2);
const MUTED: Color32 = Color32::from_rgb(0x8A, 0x93, 0xA8);
const DIM: Color32 = Color32::from_rgb(0x5C, 0x65, 0x78);
const ACCENT: Color32 = Color32::from_rgb(0x5B, 0x7C, 0xFF);
const OK: Color32 = Color32::from_rgb(0x3D, 0xDC, 0x97);
const WARN: Color32 = Color32::from_rgb(0xF0, 0x9B, 0x4B);
const ERR: Color32 = Color32::from_rgb(0xF0, 0x61, 0x6D);

/// 图标框尺寸（100% 缩放下约 50px，与 WPF 版一致）
const ICON_BOX: f32 = 50.0;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Ok,
    Info,
    Err,
}

impl Kind {
    fn color(self) -> Color32 {
        match self {
            Kind::Ok => OK,
            Kind::Info => ACCENT,
            Kind::Err => ERR,
        }
    }
}

struct Toast {
    text: String,
    kind: Kind,
    until: f64,
}

enum Dialog {
    Save { name: String },
    Delete { index: usize },
    Repair,
}

#[derive(Default)]
struct Stats {
    icons: String,
    arrange: Option<bool>,
    screen: String,
}

struct App {
    layouts: Vec<deskpos::model::LayoutEntry>,
    selected: usize,
    stats: Stats,
    status: String,
    toast: Option<Toast>,
    dialog: Option<Dialog>,

    show_ghost: bool,
    ghost: Vec<IconPos>,

    wallpaper: Option<egui::TextureHandle>,
    wallpaper_tried: bool,
    icons: IconProvider,
    textures: HashMap<String, Option<egui::TextureHandle>>,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx);

        let mut app = App {
            layouts: store::list(),
            selected: 0,
            stats: Stats::default(),
            status: String::new(),
            toast: None,
            dialog: None,
            show_ghost: false,
            ghost: Vec::new(),
            wallpaper: None,
            wallpaper_tried: false,
            icons: IconProvider::new(),
            textures: HashMap::new(),
        };
        app.refresh_stats();
        app.status = "就绪 · 已连接到桌面".to_string();
        app
    }

    // ------------------------------------------------------------ 数据
    fn current(&self) -> Option<&deskpos::model::LayoutEntry> {
        self.layouts.get(self.selected)
    }

    fn reload(&mut self) {
        let keep = self.current().map(|e| e.file_name.clone());
        self.layouts = store::list();
        self.selected = keep
            .and_then(|k| self.layouts.iter().position(|e| e.file_name == k))
            .unwrap_or(0)
            .min(self.layouts.len().saturating_sub(1));
        if self.show_ghost {
            self.load_ghost();
        }
    }

    fn refresh_stats(&mut self) {
        match deskpos::desktop::Desktop::open() {
            Ok(d) => {
                self.stats.icons = d.count().map(|c| c.to_string()).unwrap_or_else(|_| "—".into());
                self.stats.arrange = Some(d.auto_arrange());
            }
            Err(_) => {
                self.stats.icons = "—".into();
                self.stats.arrange = None;
            }
        }
        let (w, h) = deskpos::sys::screen_size();
        self.stats.screen = format!("{}×{}", w, h);
    }

    fn notify(&mut self, ctx: &egui::Context, text: impl Into<String>, kind: Kind) {
        let now = ctx.input(|i| i.time);
        self.toast = Some(Toast {
            text: text.into(),
            kind,
            until: now + 3.2,
        });
    }

    fn load_ghost(&mut self) {
        self.ghost = match self.current() {
            Some(e) => actions::ghost(&e.data).unwrap_or_default(),
            None => Vec::new(),
        };
    }

    // ------------------------------------------------------------ 动作
    fn do_save(&mut self, ctx: &egui::Context, name: &str) {
        let name = name.trim().to_string();
        if name.is_empty() {
            self.notify(ctx, "名字不能为空", Kind::Err);
            return;
        }
        match actions::snapshot(&name) {
            Ok(layout) => {
                if layout.icons.is_empty() {
                    self.notify(ctx, "桌面上没有图标，无法保存", Kind::Err);
                    return;
                }
                match store::save(&layout) {
                    Ok(_) => {
                        self.icons.invalidate();
                        self.notify(
                            ctx,
                            format!("已保存「{}」· {} 个图标", name, layout.icons.len()),
                            Kind::Ok,
                        );
                        self.reload();
                        self.selected = self
                            .layouts
                            .iter()
                            .position(|e| e.data.name == name)
                            .unwrap_or(0);
                        self.refresh_stats();
                    }
                    Err(e) => self.notify(ctx, format!("{:#}", e), Kind::Err),
                }
            }
            Err(e) => self.notify(ctx, format!("{:#}", e), Kind::Err),
        }
    }

    fn do_restore(&mut self, ctx: &egui::Context) {
        let Some(entry) = self.current() else {
            self.notify(ctx, "先选中一个布局", Kind::Info);
            return;
        };
        let data: Layout = entry.data.clone();
        let name = entry.display_name().to_string();
        let total = entry.count();

        match actions::restore(&data) {
            Ok(r) => {
                let mut msg = if r.failed.is_empty() {
                    format!("已恢复 {} 个图标", r.restored)
                } else {
                    format!("恢复了 {} 个，{} 个没摆到位", r.restored, r.failed.len())
                };
                if !r.missing.is_empty() {
                    msg.push_str(&format!(" · {} 个未找到", r.missing.len()));
                }
                let kind = if !r.failed.is_empty() {
                    Kind::Err
                } else if !r.missing.is_empty() {
                    Kind::Info
                } else {
                    Kind::Ok
                };
                self.notify(ctx, msg, kind);

                let mut parts = vec![
                    format!("布局 {} 个图标", total),
                    format!("移动 {} 个", r.moved),
                ];
                if r.auto_arrange_disabled {
                    parts.push("已关闭自动排列".into());
                }
                if !r.extra.is_empty() {
                    parts.push(format!("新增 {} 个留在原位", r.extra.len()));
                }
                if !r.missing.is_empty() {
                    parts.push(format!("已消失 {} 个", r.missing.len()));
                }
                parts.push(if r.used_shell_api {
                    "位置已写入系统，重启后保持".into()
                } else {
                    "已改内存，重启可能被系统覆盖".into()
                });
                self.status = parts.join(" · ");

                if !r.used_shell_api {
                    self.notify(ctx, "系统接口不可用，位置只在本次生效", Kind::Info);
                }
                let _ = name;
                if self.show_ghost {
                    self.load_ghost();
                }
                self.refresh_stats();
            }
            Err(e) => self.notify(ctx, format!("{:#}", e), Kind::Err),
        }
    }

    fn do_diff(&mut self, ctx: &egui::Context) {
        let Some(entry) = self.current() else { return };
        let data = entry.data.clone();
        let name = entry.display_name().to_string();
        match actions::plan(&data) {
            Ok(p) => {
                let msg = if p.to_move == 0 {
                    format!("「{}」与当前桌面完全一致", name)
                } else {
                    format!("「{}」有 {} 个图标会移动", name, p.to_move)
                };
                self.notify(ctx, msg, if p.to_move == 0 { Kind::Ok } else { Kind::Info });
                self.status = format!(
                    "布局含 {} 个图标 · 待移动 {} · 已消失 {} · 新增 {}",
                    p.total,
                    p.to_move,
                    p.missing.len(),
                    p.extra.len()
                );
            }
            Err(e) => self.notify(ctx, format!("{:#}", e), Kind::Err),
        }
    }

    fn do_delete(&mut self, ctx: &egui::Context, index: usize) {
        let Some(entry) = self.layouts.get(index) else { return };
        let name = entry.display_name().to_string();
        match store::delete_entry(entry) {
            Ok(_) => {
                self.notify(ctx, format!("已删除「{}」", name), Kind::Ok);
                self.reload();
            }
            Err(e) => self.notify(ctx, format!("{:#}", e), Kind::Err),
        }
    }

    fn do_repair(&mut self, ctx: &egui::Context) {
        match repair::repair() {
            Ok(None) => self.notify(
                ctx,
                "系统里没有 IconLayouts 缓存，已经是干净状态",
                Kind::Ok,
            ),
            Ok(Some(info)) => self.notify(
                ctx,
                format!(
                    "已备份并删除 IconLayouts（{} 字节）→ 重启后执行一次恢复即可",
                    info.bytes
                ),
                Kind::Ok,
            ),
            Err(e) => self.notify(ctx, format!("{:#}", e), Kind::Err),
        }
    }

    fn texture_of(&mut self, ctx: &egui::Context, name: &str) -> Option<egui::TextureHandle> {
        if let Some(hit) = self.textures.get(name) {
            return hit.clone();
        }
        let handle = self.icons.get(name).map(|img| {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.data);
            ctx.load_texture(name.to_string(), image, egui::TextureOptions::LINEAR)
        });
        self.textures.insert(name.to_string(), handle.clone());
        handle
    }

    fn ensure_wallpaper(&mut self, ctx: &egui::Context) {
        if self.wallpaper_tried {
            return;
        }
        self.wallpaper_tried = true;
        if let Some(img) = gui_icon::wallpaper_rgba(900) {
            let image =
                egui::ColorImage::from_rgba_unmultiplied([img.w as usize, img.h as usize], &img.data);
            self.wallpaper = Some(ctx.load_texture("wallpaper", image, egui::TextureOptions::LINEAR));
        }
    }

    // ------------------------------------------------------------ UI
    fn top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("top")
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(16, 12)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("DeskPos").size(19.0).strong().color(TEXT));
                    ui.label(RichText::new("桌面布局 · Rust").size(12.0).color(DIM));
                    ui.add_space(18.0);

                    chip(ui, "图标", &self.stats.icons, TEXT);
                    let (arrange_text, color) = match self.stats.arrange {
                        Some(true) => ("开启", WARN),
                        Some(false) => ("已关闭", OK),
                        None => ("—", MUTED),
                    };
                    chip(ui, "自动排列", arrange_text, color);
                    chip(ui, "分辨率", &self.stats.screen, TEXT);

                    ui.with_layout(EguiLayout::right_to_left(Align::Center), |ui| {
                        if ui.button("刷新").clicked() {
                            self.reload();
                            self.refresh_stats();
                            self.icons.invalidate();
                            self.wallpaper_tried = false;
                            self.wallpaper = None;
                        }
                        if ui.button("打开目录").clicked() {
                            let _ = std::process::Command::new("explorer.exe")
                                .arg(store::dir())
                                .spawn();
                        }
                        if ui.button("修复布局缓存").clicked() {
                            self.dialog = Some(Dialog::Repair);
                        }
                    });
                });
            });
    }

    fn side_panel(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("side")
            .exact_width(312.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(Color32::from_rgb(0x11, 0x14, 0x1C)).inner_margin(egui::Margin::symmetric(14, 14)))
            .show(ctx, |ui| {
                let w = ui.available_width();
                let save_btn = egui::Button::new(
                    RichText::new("＋  保存当前桌面").size(13.5).color(Color32::WHITE),
                )
                .fill(ACCENT)
                .min_size(Vec2::new(w, 34.0));
                if ui.add(save_btn).clicked() {
                    self.dialog = Some(Dialog::Save {
                        name: chrono::Local::now().format("%m-%d %H:%M").to_string(),
                    });
                }
                ui.add_space(10.0);
                ui.label(RichText::new("已保存的布局").size(11.5).color(DIM));
                ui.add_space(6.0);

                // 底部按钮区先留出高度
                let bottom_h = 76.0;
                let list_h = (ui.available_height() - bottom_h).max(80.0);

                if self.layouts.is_empty() {
                    ui.add_space(20.0);
                    ui.label(RichText::new("还没有布局\n点上面的按钮存一份吧").size(12.5).color(MUTED));
                } else {
                    egui::ScrollArea::vertical()
                        .max_height(list_h)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let rows: Vec<(String, String, String, bool)> = self
                                .layouts
                                .iter()
                                .enumerate()
                                .map(|(i, e)| {
                                    (
                                        e.display_name().to_string(),
                                        format!("{} 个图标 · {}", e.count(), e.data.screen_text()),
                                        e.stamp.clone(),
                                        i == self.selected,
                                    )
                                })
                                .collect();

                            for (i, (name, meta, stamp, selected)) in rows.into_iter().enumerate() {
                                let (rect, resp) = ui.allocate_exact_size(
                                    Vec2::new(ui.available_width(), 60.0),
                                    Sense::click(),
                                );
                                let p = ui.painter();
                                let bg = if selected {
                                    CARD_SEL
                                } else if resp.hovered() {
                                    Color32::from_rgb(0x1A, 0x1F, 0x29)
                                } else {
                                    CARD
                                };
                                p.rect_filled(rect.shrink2(Vec2::new(0.0, 2.0)), CornerRadius::same(7), bg);
                                if selected {
                                    p.rect_stroke(
                                        rect.shrink2(Vec2::new(0.0, 2.0)),
                                        CornerRadius::same(7),
                                        Stroke::new(1.0_f32, ACCENT),
                                        StrokeKind::Inside,
                                    );
                                }
                                let base = rect.left_top() + Vec2::new(11.0, 8.0);
                                p.text(
                                    base,
                                    Align2::LEFT_TOP,
                                    &name,
                                    FontId::proportional(13.5),
                                    TEXT,
                                );
                                p.text(
                                    base + Vec2::new(0.0, 20.0),
                                    Align2::LEFT_TOP,
                                    &meta,
                                    FontId::proportional(11.0),
                                    MUTED,
                                );
                                p.text(
                                    base + Vec2::new(0.0, 35.0),
                                    Align2::LEFT_TOP,
                                    &stamp,
                                    FontId::proportional(10.0),
                                    DIM,
                                );

                                if resp.clicked() {
                                    self.selected = i;
                                    if self.show_ghost {
                                        self.load_ghost();
                                    }
                                }
                                if resp.double_clicked() {
                                    self.selected = i;
                                    self.do_restore(ctx);
                                }
                                ui.add_space(2.0);
                            }
                        });
                }

                // 底部操作
                ui.add_space(6.0);
                let has = !self.layouts.is_empty();
                let half = (w - 8.0) / 2.0;
                ui.horizontal(|ui| {
                    let restore = egui::Button::new(
                        RichText::new("恢复选中").size(13.0).color(if has { Color32::WHITE } else { DIM }),
                    )
                    .fill(if has { Color32::from_rgb(0x2A, 0x37, 0x5E) } else { CARD })
                    .min_size(Vec2::new(half, 30.0));
                    if ui.add_enabled(has, restore).clicked() {
                        self.do_restore(ctx);
                    }
                    let diff = egui::Button::new(RichText::new("预览差异").size(13.0).color(MUTED))
                        .fill(CARD)
                        .min_size(Vec2::new(half, 30.0));
                    if ui.add_enabled(has, diff).clicked() {
                        self.do_diff(ctx);
                    }
                });
                ui.horizontal(|ui| {
                    let del = egui::Button::new(RichText::new("删除").size(13.0).color(MUTED))
                        .fill(CARD)
                        .min_size(Vec2::new(half, 30.0));
                    if ui.add_enabled(has, del).clicked() {
                        self.dialog = Some(Dialog::Delete {
                            index: self.selected,
                        });
                    }
                    let ghost_label = if self.show_ghost { "隐藏当前位置" } else { "叠加当前位置" };
                    let ghost = egui::Button::new(RichText::new(ghost_label).size(13.0).color(
                        if self.show_ghost {
                            Color32::from_rgb(0x9C, 0xB0, 0xFF)
                        } else {
                            MUTED
                        },
                    ))
                    .fill(if self.show_ghost { CARD_SEL } else { CARD })
                    .min_size(Vec2::new(half, 30.0));
                    if ui.add_enabled(has, ghost).clicked() {
                        self.show_ghost = !self.show_ghost;
                        if self.show_ghost {
                            self.load_ghost();
                            let n = self.ghost.len();
                            if n == 0 {
                                self.notify(
                                    ctx,
                                    "这份布局和当前桌面完全一致，没有需要移动的图标",
                                    Kind::Ok,
                                );
                            } else {
                                self.notify(
                                    ctx,
                                    format!("蓝色虚线是 {} 个图标现在的位置", n),
                                    Kind::Info,
                                );
                            }
                        } else {
                            self.ghost.clear();
                        }
                    }
                });
            });
    }

    fn bottom_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status")
            .frame(egui::Frame::new().fill(Color32::from_rgb(0x0B, 0x0D, 0x12)).inner_margin(egui::Margin::symmetric(16, 8)))
            .show(ctx, |ui| {
                let now = ctx.input(|i| i.time);
                let toast = self.toast.as_ref().filter(|t| t.until > now);
                ui.horizontal(|ui| {
                    if let Some(t) = toast {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 3.5, t.kind.color());
                        ui.label(RichText::new(&t.text).size(12.0).color(t.kind.color()));
                    } else {
                        ui.label(RichText::new(&self.status).size(12.0).color(MUTED));
                    }
                    ui.with_layout(EguiLayout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{}", store::dir().display()))
                                .size(10.5)
                                .color(DIM),
                        );
                    });
                });
            });
    }

    fn center_panel(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::same(14)))
            .show(ctx, |ui| {
                if self.layouts.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("左侧还没有布局\n点「保存当前桌面」记录一份")
                                .size(13.0)
                                .color(DIM),
                        );
                    });
                    return;
                }

                let (title, meta) = match self.current() {
                    Some(e) => (
                        e.display_name().to_string(),
                        format!("{} 个图标 · {}", e.count(), e.data.screen_text()),
                    ),
                    None => (String::new(), String::new()),
                };

                ui.horizontal(|ui| {
                    ui.label(RichText::new(title).size(16.0).strong().color(TEXT));
                    ui.add_space(8.0);
                    ui.label(RichText::new(meta).size(11.5).color(MUTED));
                });
                ui.add_space(8.0);

                let layout = self.current().map(|e| e.data.clone());
                if let Some(layout) = layout {
                    self.preview(ui, &layout);
                }
            });
    }

    fn preview(&mut self, ui: &mut egui::Ui, layout: &Layout) {
        self.ensure_wallpaper(ui.ctx());

        let size = ui.available_size();
        if size.x < 60.0 || size.y < 60.0 {
            return;
        }
        let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, CornerRadius::same(10), Color32::from_rgb(0x0C, 0x0E, 0x13));

        if layout.icons.is_empty() {
            return;
        }

        let c = Canvas::new(rect, layout);
        let screen = c.screen_rect;

        // ---- 壁纸
        if let Some(tex) = &self.wallpaper {
            let uv = cover_uv(tex.size_vec2(), screen);
            painter.image(tex.id(), screen, uv, Color32::WHITE);
            painter.rect_filled(screen, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(0x08, 0x0A, 0x10, 0x16));
        } else {
            // 没有壁纸就用蓝紫渐变兜底
            let steps = 28;
            for i in 0..steps {
                let t0 = i as f32 / steps as f32;
                let t1 = (i + 1) as f32 / steps as f32;
                let band = Rect::from_min_max(
                    Pos2::new(screen.left(), screen.top() + screen.height() * t0),
                    Pos2::new(screen.right(), screen.top() + screen.height() * t1 + 1.0),
                );
                painter.rect_filled(band, CornerRadius::ZERO, mix(Color32::from_rgb(0x17, 0x25, 0x45), Color32::from_rgb(0x23, 0x1B, 0x3E), t0));
            }
        }

        // ---- 当前位置叠加（只标会动的）
        let box_side = ICON_BOX * c.scale;
        if self.show_ghost {
            for g in &self.ghost {
                let r = Rect::from_min_size(c.pos(g.x as f32, g.y as f32), Vec2::splat(box_side));
                dashed_rect(
                    &painter,
                    r,
                    Stroke::new(1.3_f32, Color32::from_rgba_unmultiplied(0x6F, 0x8C, 0xFF, 0xE6)),
                    4.0,
                    3.0,
                );
            }
        }

        // ---- 图标 + 悬停命中
        let pointer = resp.hover_pos();
        let mut hovered: Option<(String, Rect)> = None;
        for it in layout.icons.iter().rev() {
            let r = Rect::from_min_size(c.pos(it.x as f32, it.y as f32), Vec2::splat(box_side));
            if let Some(p) = pointer {
                if r.contains(p) && hovered.is_none() {
                    hovered = Some((it.name.clone(), r));
                }
            }
        }

        for it in &layout.icons {
            let r = Rect::from_min_size(c.pos(it.x as f32, it.y as f32), Vec2::splat(box_side));
            if let Some((name, _)) = &hovered {
                if name == &it.name {
                    painter.rect_filled(
                        r.expand(1.0),
                        CornerRadius::same(4),
                        Color32::from_rgba_unmultiplied(0x5B, 0x7C, 0xFF, 0x30),
                    );
                }
            }

            match self.texture_of(ui.ctx(), &it.name) {
                Some(tex) => {
                    painter.image(tex.id(), r, uv_full(), Color32::WHITE);
                }
                None => {
                    let (rr, gg, bb) = gui_icon::color_of(&it.name);
                    painter.rect_filled(
                        r,
                        CornerRadius::same(3),
                        Color32::from_rgb(rr, gg, bb),
                    );
                    let first = it.name.chars().next().unwrap_or('?').to_string();
                    painter.text(
                        r.center(),
                        Align2::CENTER_CENTER,
                        first,
                        FontId::proportional((box_side * 0.55).max(7.0)),
                        Color32::WHITE,
                    );
                }
            }
        }

        // ---- 任务栏
        draw_taskbar(&painter, screen, c.scale);

        // ---- 屏幕边框
        painter.rect_stroke(
            screen,
            CornerRadius::same(6),
            Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0x64, 0x74, 0x94, 0x66)),
            StrokeKind::Inside,
        );

        // ---- 悬停名称气泡
        if let Some((name, icon_rect)) = hovered {
            draw_tooltip(&painter, rect, icon_rect, &name);
        }
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        let mut close = false;

        match &mut self.dialog {
            Some(Dialog::Save { name }) => {
                let mut buf = name.clone();
                let mut confirm = false;
                egui::Window::new("保存当前布局")
                    .frame(dialog_frame())
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.set_width(320.0);
                        ui.label(RichText::new("给它起个名字：").size(12.5).color(MUTED));
                        ui.add_space(6.0);
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut buf)
                                .desired_width(f32::INFINITY)
                                .hint_text("例如 工作 / 双屏"),
                        );
                        r.request_focus();
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let ok = egui::Button::new(RichText::new("保存").color(Color32::WHITE))
                                .fill(ACCENT)
                                .min_size(Vec2::new(120.0, 30.0));
                            if ui.add(ok).clicked() {
                                confirm = true;
                            }
                            if ui.button("取消").clicked() {
                                close = true;
                            }
                        });
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            confirm = true;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            close = true;
                        }
                    });
                *name = buf;
                if confirm {
                    let n = name.clone();
                    self.dialog = None;
                    self.do_save(ctx, &n);
                }
            }
            Some(Dialog::Delete { index }) => {
                let idx = *index;
                let title = self
                    .layouts
                    .get(idx)
                    .map(|e| e.display_name().to_string())
                    .unwrap_or_default();
                let mut confirm = false;
                egui::Window::new("删除布局")
                    .frame(dialog_frame())
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.set_width(320.0);
                        ui.label(
                            RichText::new(format!("确定要删除「{}」吗？此操作不可撤销。", title))
                                .size(12.5)
                                .color(TEXT),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let ok = egui::Button::new(RichText::new("删除").color(Color32::WHITE))
                                .fill(Color32::from_rgb(0x8C, 0x2F, 0x38))
                                .min_size(Vec2::new(120.0, 30.0));
                            if ui.add(ok).clicked() {
                                confirm = true;
                            }
                            if ui.button("取消").clicked() {
                                close = true;
                            }
                        });
                    });
                if confirm {
                    self.dialog = None;
                    self.do_delete(ctx, idx);
                }
            }
            Some(Dialog::Repair) => {
                let mut confirm = false;
                egui::Window::new("修复布局缓存")
                    .frame(dialog_frame())
                    .collapsible(false)
                    .resizable(false)
                    .anchor(Align2::CENTER_CENTER, Vec2::ZERO)
                    .show(ctx, |ui| {
                        ui.set_width(360.0);
                        ui.label(
                            RichText::new(
                                "只在「每次重启都固定错乱同一批图标、拖回去也没用」时才需要。\n\n\
                                 会先备份，再删掉注册表里的 IconLayouts 缓存，重启后系统重建一份干净的，\
                                 然后执行一次恢复即可。",
                            )
                            .size(12.5)
                            .color(TEXT),
                        );
                        ui.add_space(12.0);
                        ui.horizontal(|ui| {
                            let ok = egui::Button::new(RichText::new("备份并清理").color(Color32::WHITE))
                                .fill(Color32::from_rgb(0x8C, 0x5A, 0x2F))
                                .min_size(Vec2::new(130.0, 30.0));
                            if ui.add(ok).clicked() {
                                confirm = true;
                            }
                            if ui.button("取消").clicked() {
                                close = true;
                            }
                        });
                    });
                if confirm {
                    self.dialog = None;
                    self.do_repair(ctx);
                }
            }
            None => {}
        }

        if close {
            self.dialog = None;
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        apply_theme(ctx);
        self.top_bar(ctx);
        self.side_panel(ctx);
        self.bottom_bar(ctx);
        self.center_panel(ctx);
        self.dialogs(ctx);

        if let Some(t) = &self.toast {
            if t.until > ctx.input(|i| i.time) {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
    }
}

// ---------------------------------------------------------------- 画布换算
struct Canvas {
    screen_rect: Rect,
    scale: f32,
    ox: f32,
    oy: f32,
}

impl Canvas {
    fn new(area: Rect, layout: &Layout) -> Self {
        let sw = if layout.screen.w > 0 { layout.screen.w as f32 } else { 1920.0 };
        let sh = if layout.screen.h > 0 { layout.screen.h as f32 } else { 1080.0 };

        // 按坐标范围扩展画布，兼容主屏左侧的负坐标副屏
        let mut min_x = 0.0f32;
        let mut min_y = 0.0f32;
        let mut max_x = sw;
        let mut max_y = sh;
        for it in &layout.icons {
            let x = it.x as f32;
            let y = it.y as f32;
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x + ICON_BOX);
            max_y = max_y.max(y + ICON_BOX);
        }

        let vw = (max_x - min_x).max(1.0);
        let vh = (max_y - min_y).max(1.0);
        const PAD: f32 = 9.0;
        let scale = (((area.width() - PAD * 2.0) / vw).min((area.height() - PAD * 2.0) / vh)).max(0.02);
        let ox = area.left() + (area.width() - vw * scale) / 2.0 - min_x * scale;
        let oy = area.top() + (area.height() - vh * scale) / 2.0 - min_y * scale;

        Canvas {
            screen_rect: Rect::from_min_max(
                Pos2::new(ox, oy),
                Pos2::new(ox + sw * scale, oy + sh * scale),
            ),
            scale,
            ox,
            oy,
        }
    }

    fn pos(&self, x: f32, y: f32) -> Pos2 {
        Pos2::new(self.ox + x * self.scale, self.oy + y * self.scale)
    }
}

fn uv_full() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0))
}

/// 等比裁切铺满（相当于 WPF 的 Stretch.UniformToFill）
fn cover_uv(tex: Vec2, dst: Rect) -> Rect {
    let mut uv = uv_full();
    if tex.x <= 0.0 || tex.y <= 0.0 || dst.height() <= 0.0 {
        return uv;
    }
    let tex_ar = tex.x / tex.y;
    let dst_ar = dst.width() / dst.height();
    if tex_ar > dst_ar {
        let frac = dst_ar / tex_ar;
        let dx = (1.0 - frac) / 2.0;
        uv.min.x = dx;
        uv.max.x = 1.0 - dx;
    } else {
        let frac = tex_ar / dst_ar;
        let dy = (1.0 - frac) / 2.0;
        uv.min.y = dy;
        uv.max.y = 1.0 - dy;
    }
    uv
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn dashed_rect(painter: &egui::Painter, r: Rect, stroke: Stroke, dash: f32, gap: f32) {
    let pts = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    for w in pts.windows(2) {
        painter.extend(egui::Shape::dashed_line(&[w[0], w[1]], stroke, dash, gap));
    }
}

fn draw_taskbar(painter: &egui::Painter, screen: Rect, scale: f32) {
    let tb = (52.0 * scale).max(4.0);
    let bar = Rect::from_min_max(
        Pos2::new(screen.left(), screen.bottom() - tb),
        Pos2::new(screen.right(), screen.bottom()),
    );
    painter.rect_filled(bar, CornerRadius::ZERO, Color32::from_rgba_unmultiplied(0x0A, 0x0D, 0x14, 0xF0));
    painter.line_segment(
        [
            Pos2::new(bar.left(), bar.top()),
            Pos2::new(bar.right(), bar.top()),
        ],
        Stroke::new(scale.max(0.6), Color32::from_rgba_unmultiplied(0x4A, 0x58, 0x76, 0x55)),
    );

    let cy = bar.center().y;
    // 开始按钮
    let s = tb * 0.42;
    let start = Rect::from_min_size(
        Pos2::new(bar.left() + tb * 0.28, cy - s / 2.0),
        Vec2::splat(s),
    );
    painter.rect_filled(start, CornerRadius::same((s * 0.22) as u8), Color32::from_rgb(0x5B, 0x7C, 0xFF));

    // 搜索框
    let search = Rect::from_min_size(
        Pos2::new(start.right() + tb * 0.34, cy - tb * 0.19),
        Vec2::new(screen.width() * 0.155, tb * 0.38),
    );
    painter.rect_filled(
        search,
        CornerRadius::same((tb * 0.16) as u8),
        Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 0x1E),
    );

    // 固定的程序
    let mut ax = search.right() + tb * 0.5;
    for i in 0..4 {
        let asz = tb * 0.4;
        let a = Rect::from_min_size(Pos2::new(ax, cy - asz / 2.0), Vec2::splat(asz));
        if a.right() > screen.right() - tb * 3.4 {
            break;
        }
        let alpha = if i == 0 { 0x40 } else { 0x1C };
        painter.rect_filled(
            a,
            CornerRadius::same((asz * 0.2) as u8),
            Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, alpha),
        );
        ax += tb * 0.62;
    }

    // 时间 / 通知
    let clock = Rect::from_min_size(
        Pos2::new(
            screen.right() - tb * 0.42 - screen.width() * 0.045,
            cy - tb * 0.19,
        ),
        Vec2::new(screen.width() * 0.045, tb * 0.38),
    );
    painter.rect_filled(
        clock,
        CornerRadius::same((tb * 0.16) as u8),
        Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 0x1A),
    );
}

fn draw_tooltip(painter: &egui::Painter, area: Rect, anchor: Rect, name: &str) {
    let galley = painter.layout_no_wrap(name.to_string(), FontId::proportional(11.5), Color32::WHITE);
    let tw = galley.size().x.min(230.0) + 16.0;
    let th = galley.size().y + 9.0;

    let mut x = anchor.center().x - tw / 2.0;
    let mut y = anchor.bottom() + 4.0;
    if y + th > area.bottom() - 4.0 {
        y = anchor.top() - th - 4.0;
    }
    x = x.max(area.left() + 4.0).min(area.right() - tw - 4.0);
    y = y.max(area.top() + 4.0).min(area.bottom() - th - 4.0);

    let pill = Rect::from_min_size(Pos2::new(x, y), Vec2::new(tw, th));
    painter.rect_filled(pill, CornerRadius::same(5), Color32::from_rgba_unmultiplied(0x1B, 0x20, 0x2C, 0xF2));
    painter.rect_stroke(
        pill,
        CornerRadius::same(5),
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(0x3E, 0x49, 0x60, 0xAA)),
        StrokeKind::Inside,
    );
    painter.galley(
        Pos2::new(pill.left() + 8.0, pill.center().y - galley.size().y / 2.0),
        galley,
        TEXT,
    );
}

fn chip(ui: &mut egui::Ui, label: &str, value: &str, color: Color32) {
    let text = format!("{} {}", label, if value.is_empty() { "—" } else { value });
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.painter().layout_no_wrap(text.clone(), FontId::proportional(11.5), color).size().x + 18.0, 22.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(11), Color32::from_rgb(0x18, 0x1D, 0x27));
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(11.5), color);
    ui.add_space(6.0);
}

fn dialog_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgb(0x16, 0x1A, 0x24))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(0x2E, 0x36, 0x48)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(egui::Margin::same(16))
}

fn dark_visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = Color32::from_rgb(0x16, 0x1A, 0x24);
    v.extreme_bg_color = Color32::from_rgb(0x0A, 0x0C, 0x11);
    v.faint_bg_color = Color32::from_rgb(0x14, 0x18, 0x21);
    v.override_text_color = Some(TEXT);
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0x24, 0x2A, 0x38));
    v.widgets.noninteractive.bg_fill = CARD;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);
    v.widgets.inactive.bg_fill = CARD;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    v.widgets.inactive.weak_bg_fill = CARD;
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x24, 0x2C, 0x3C);
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x24, 0x2C, 0x3C);
    v.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.widgets.active.bg_fill = Color32::from_rgb(0x2C, 0x36, 0x4A);
    v.widgets.active.weak_bg_fill = Color32::from_rgb(0x2C, 0x36, 0x4A);
    v.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.selection.bg_fill = Color32::from_rgb(0x2A, 0x3A, 0x6B);
    v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    v.window_stroke = Stroke::new(1.0_f32, Color32::from_rgb(0x2B, 0x31, 0x40));
    v.hyperlink_color = ACCENT;
    v
}

/// 强制深色：egui 的深/浅色主题会随系统切换，每帧都盖一次最省心
fn apply_theme(ctx: &egui::Context) {
    ctx.options_mut(|o| o.theme_preference = egui::ThemePreference::Dark);
    ctx.all_styles_mut(|s| {
        s.visuals = dark_visuals();
        s.spacing.item_spacing = Vec2::new(8.0, 6.0);
        s.spacing.button_padding = Vec2::new(10.0, 5.0);
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(12.5));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(12.5));
    });
}

/// egui 自带字体没有中文，从系统里借一份
fn install_fonts(ctx: &egui::Context) {
    const CANDIDATES: [&str; 4] = [
        "C:/Windows/Fonts/Deng.ttf",   // 等线
        "C:/Windows/Fonts/simhei.ttf", // 黑体
        "C:/Windows/Fonts/msyh.ttc",   // 微软雅黑
        "C:/Windows/Fonts/simsun.ttc", // 宋体
    ];

    let mut fonts = egui::FontDefinitions::default();
    for path in CANDIDATES {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        fonts
            .font_data
            .insert("cjk".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            list.insert(0, "cjk".to_owned());
        }
        if let Some(list) = fonts.families.get_mut(&egui::FontFamily::Monospace) {
            list.push("cjk".to_owned());
        }
        break;
    }
    ctx.set_fonts(fonts);
}

/// 按屏幕工作区自动挑一个放得下的窗口尺寸（125% / 150% 缩放下也不会跑出屏幕）
fn initial_viewport() -> egui::ViewportBuilder {
    let (_, _, aw, ah) = deskpos::sys::work_area();
    let scale = deskpos::sys::dpi_scale().max(0.5);
    let w = (aw as f32 / scale - 60.0).clamp(960.0, 1280.0);
    let h = (ah as f32 / scale - 76.0).clamp(600.0, 820.0);
    egui::ViewportBuilder::default()
        .with_inner_size([w, h])
        .with_min_inner_size([900.0, 540.0])
        .with_position([24.0, 16.0])
        .with_title("DeskPos · 桌面布局")
}

fn main() -> eframe::Result<()> {
    deskpos::init();

    let options = eframe::NativeOptions {
        viewport: initial_viewport(),
        ..Default::default()
    };

    eframe::run_native(
        "DeskPos",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
