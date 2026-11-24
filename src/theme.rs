//! Design tokens (see DESIGN.md) and small painter helpers.

use eframe::egui::{
    self, text::LayoutJob, Color32, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2,
    Rect, Shape, Stroke, TextFormat, Vec2,
};
use std::sync::Arc;

pub const VOID: Color32 = Color32::from_rgb(0x0A, 0x0B, 0x0D);
pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x14, 0x18);
pub const RAISED: Color32 = Color32::from_rgb(0x1A, 0x1D, 0x22);
pub const GRID: Color32 = Color32::from_rgb(0x2A, 0x2F, 0x37);
pub const LINE: Color32 = Color32::from_rgb(0x30, 0x36, 0x3E);
pub const ASH: Color32 = Color32::from_rgb(0x8A, 0x90, 0x99);
pub const BONE: Color32 = Color32::from_rgb(0xEC, 0xE8, 0xDD);
pub const ACID: Color32 = Color32::from_rgb(0xD4, 0xFF, 0x1F);
pub const FLARE: Color32 = Color32::from_rgb(0xFF, 0x2E, 0x88);
pub const EMBER: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x1A);
pub const ION: Color32 = Color32::from_rgb(0x2E, 0xF2, 0xFF);
pub const UV: Color32 = Color32::from_rgb(0x9D, 0x6B, 0xFF);

pub const ACCENTS: [Color32; 6] = [ACID, FLARE, ION, EMBER, UV, BONE];
pub const ACCENT_NAMES: [&str; 6] = ["ACID", "FLARE", "ION", "EMBER", "UV", "BONE"];

pub fn accent(i: u8) -> Color32 {
    ACCENTS[i as usize % ACCENTS.len()]
}

pub const BUBBLE_R: f32 = 26.0;
pub const PLATE_H: f32 = 34.0;

/// Every distinct font size gets its own glyphs in the font atlas, and zoom
/// multiplies sizes by arbitrary factors. Snapping to whole points keeps the
/// number of sizes (and so memory) bounded no matter how much you zoom.
fn snap(size: f32) -> f32 {
    size.round().clamp(5.0, 64.0)
}
pub fn display(size: f32) -> FontId {
    FontId::new(snap(size), FontFamily::Name("display".into()))
}
pub fn mono(size: f32) -> FontId {
    FontId::monospace(if size < 12.0 { (size * 2.0).round() / 2.0 } else { snap(size) }.max(5.0))
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "oxanium".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Oxanium-Bold.ttf"))),
    );
    fonts.font_data.insert(
        "jbmono".into(),
        Arc::new(FontData::from_static(include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf"))),
    );
    let mono_chain = {
        let mut v = vec!["jbmono".to_string()];
        v.extend(fonts.families.get(&FontFamily::Monospace).cloned().unwrap_or_default());
        v
    };
    let mut display_chain = vec!["oxanium".to_string()];
    display_chain.extend(mono_chain.iter().cloned());
    fonts.families.insert(FontFamily::Monospace, mono_chain.clone());
    fonts.families.insert(FontFamily::Proportional, mono_chain);
    fonts.families.insert(FontFamily::Name("display".into()), display_chain);
    ctx.set_fonts(fonts);

    ctx.global_style_mut(|style| {
        use egui::TextStyle::*;
        style.text_styles = [
            (Heading, display(16.0)),
            (Body, mono(12.0)),
            (Button, mono(12.0)),
            (Monospace, mono(12.0)),
            (Small, mono(10.0)),
        ]
        .into();
        style.spacing.item_spacing = Vec2::new(8.0, 4.0);
        style.spacing.button_padding = Vec2::new(10.0, 4.0);
        style.spacing.menu_margin = egui::Margin::same(4);
        let v = &mut style.visuals;
        *v = egui::Visuals::dark();
        v.panel_fill = PANEL;
        v.window_fill = PANEL;
        v.extreme_bg_color = RAISED;
        v.faint_bg_color = RAISED;
        v.window_stroke = Stroke::new(1.0, LINE);
        v.window_corner_radius = 0.into();
        v.menu_corner_radius = 0.into();
        v.window_shadow = egui::epaint::Shadow::NONE;
        v.popup_shadow = egui::epaint::Shadow::NONE;
        v.selection.bg_fill = ACID;
        v.selection.stroke = Stroke::new(1.0, VOID);
        v.hyperlink_color = ACID;
        v.text_cursor.stroke = Stroke::new(2.0, ACID);
        v.override_text_color = Some(BONE);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = 0.into();
            w.expansion = 0.0;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, BONE);
        v.widgets.inactive.bg_fill = PANEL;
        v.widgets.inactive.weak_bg_fill = PANEL;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, BONE);
        v.widgets.hovered.bg_fill = ACID;
        v.widgets.hovered.weak_bg_fill = ACID;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACID);
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, VOID);
        v.widgets.active.bg_fill = BONE;
        v.widgets.active.weak_bg_fill = BONE;
        v.widgets.active.bg_stroke = Stroke::new(1.0, BONE);
        v.widgets.active.fg_stroke = Stroke::new(1.0, VOID);
        v.widgets.open.weak_bg_fill = RAISED;
    });
}

/// Rect with 45° cuts on the top-left and bottom-right corners.
pub fn chamfer_pts(r: Rect, c: f32) -> Vec<Pos2> {
    let c = c.min(r.width() / 2.0).min(r.height() / 2.0);
    vec![
        Pos2::new(r.min.x + c, r.min.y),
        Pos2::new(r.max.x, r.min.y),
        Pos2::new(r.max.x, r.max.y - c),
        Pos2::new(r.max.x - c, r.max.y),
        Pos2::new(r.min.x, r.max.y),
        Pos2::new(r.min.x, r.min.y + c),
    ]
}

pub fn chamfer_fill(p: &Painter, r: Rect, c: f32, fill: Color32) {
    p.add(Shape::convex_polygon(chamfer_pts(r, c), fill, Stroke::NONE));
}

pub fn chamfer_stroke(p: &Painter, r: Rect, c: f32, stroke: Stroke) {
    p.add(Shape::closed_line(chamfer_pts(r, c), stroke));
}

/// Single-line, letter-spaced, uppercase text layout.
pub fn spaced(text: &str, font: FontId, color: Color32, spacing: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.append(
        &text.to_uppercase(),
        0.0,
        TextFormat { font_id: font, color, extra_letter_spacing: spacing, ..Default::default() },
    );
    job.wrap.max_rows = 1;
    job
}

pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    c.gamma_multiply(a.clamp(0.0, 1.0))
}

/// Small "+" registration mark.
pub fn reg_mark(p: &Painter, at: Pos2, s: f32, stroke: Stroke) {
    p.line_segment([at - Vec2::X * s, at + Vec2::X * s], stroke);
    p.line_segment([at - Vec2::Y * s, at + Vec2::Y * s], stroke);
}

/// Corner brackets around a rect.
pub fn brackets(p: &Painter, r: Rect, len: f32, stroke: Stroke) {
    let pts = [
        (r.left_top(), Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)),
        (r.right_top(), Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0)),
        (r.left_bottom(), Vec2::new(1.0, 0.0), Vec2::new(0.0, -1.0)),
        (r.right_bottom(), Vec2::new(-1.0, 0.0), Vec2::new(0.0, -1.0)),
    ];
    for (c, a, b) in pts {
        p.line_segment([c, c + a * len], stroke);
        p.line_segment([c, c + b * len], stroke);
    }
}
