use crate::config::{
    entries_to_config, entries_to_toml, parse_file, ActivationKey, CaptureResult, Config,
    MatchEntry, Theme,
};
use eframe::egui;
use egui::{Color32, CornerRadius, Margin, RichText, Stroke, Vec2};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tray_icon::menu::{Menu, MenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

/// A full set of theme colors in the xAI idiom: a near-black canvas, white as
/// the "primary" color, charcoal cards, and translucent-white pill borders.
/// Light is a clean inverse so the theme toggle still works.
#[derive(Clone, Copy)]
struct Palette {
    dark: bool,
    canvas: Color32,  // the single page surface (#0a0a0a)
    sidebar: Color32, // left rail (same canvas in the xAI idiom)
    card: Color32,    // card / input fill (#191919)
    faint: Color32,
    hover: Color32,
    pressed: Color32,
    selected: Color32,    // active list row
    hairline: Color32,    // 1px solid dividers / card borders
    pill_border: Color32, // translucent border on outline pills
    button: Color32,
    focus: Color32,
    accent: Color32,    // the "primary" — white on dark, near-black on light
    on_accent: Color32, // text on the filled primary pill
    text: Color32,      // ink
    body: Color32,      // secondary copy
    muted: Color32,     // captions / eyebrows / fine print
    success: Color32,
    warning: Color32,
}

impl Palette {
    fn dark() -> Self {
        Self {
            dark: true,
            canvas: Color32::from_rgb(0x0A, 0x0A, 0x0A),
            sidebar: Color32::from_rgb(0x0A, 0x0A, 0x0A),
            card: Color32::from_rgb(0x19, 0x19, 0x19),
            faint: Color32::from_rgb(0x14, 0x14, 0x14),
            hover: Color32::from_rgb(0x1A, 0x1C, 0x20),
            pressed: Color32::from_rgb(0x23, 0x24, 0x27),
            selected: Color32::from_rgb(0x1F, 0x1F, 0x1F),
            hairline: Color32::from_rgb(0x21, 0x23, 0x27),
            pill_border: Color32::from_rgba_unmultiplied(0xFF, 0xFF, 0xFF, 56),
            button: Color32::from_rgb(0x19, 0x19, 0x19),
            focus: Color32::from_rgb(0x3A, 0x3D, 0x44),
            accent: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            on_accent: Color32::from_rgb(0x0A, 0x0A, 0x0A),
            text: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            body: Color32::from_rgb(0xDA, 0xDB, 0xDF),
            muted: Color32::from_rgb(0x7D, 0x81, 0x87),
            success: Color32::from_rgb(0x4F, 0xA5, 0x6B),
            warning: Color32::from_rgb(0xE0, 0xA3, 0x3A),
        }
    }

    fn light() -> Self {
        Self {
            dark: false,
            canvas: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            sidebar: Color32::from_rgb(0xFA, 0xFA, 0xFA),
            card: Color32::from_rgb(0xF6, 0xF6, 0xF6),
            faint: Color32::from_rgb(0xF2, 0xF2, 0xF2),
            hover: Color32::from_rgb(0xF0, 0xF0, 0xF0),
            pressed: Color32::from_rgb(0xE6, 0xE6, 0xE6),
            selected: Color32::from_rgb(0xEC, 0xEC, 0xEC),
            hairline: Color32::from_rgb(0xE3, 0xE3, 0xE3),
            pill_border: Color32::from_rgba_unmultiplied(0x0A, 0x0A, 0x0A, 48),
            button: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            focus: Color32::from_rgb(0xC9, 0xCA, 0xCE),
            accent: Color32::from_rgb(0x0A, 0x0A, 0x0A),
            on_accent: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            text: Color32::from_rgb(0x0A, 0x0A, 0x0A),
            body: Color32::from_rgb(0x3A, 0x3A, 0x3A),
            muted: Color32::from_rgb(0x6B, 0x6F, 0x76),
            success: Color32::from_rgb(0x3D, 0x8B, 0x57),
            warning: Color32::from_rgb(0xB9, 0x75, 0x19),
        }
    }

    fn for_theme(t: Theme) -> Self {
        match t {
            Theme::Light => Self::light(),
            Theme::Dark => Self::dark(),
        }
    }

    /// The rare filled pill (white on dark) used for the single primary action.
    fn primary_pill(&self, text: &str) -> egui::Button<'static> {
        egui::Button::new(
            RichText::new(text.to_owned())
                .color(self.on_accent)
                .size(13.0)
                .strong(),
        )
        .fill(self.accent)
        .stroke(Stroke::new(1.0, self.accent))
        .corner_radius(CornerRadius::same(255))
    }

    /// The canonical outline pill (transparent fill, translucent border).
    fn outline_pill(&self, text: &str, color: Color32) -> egui::Button<'static> {
        egui::Button::new(RichText::new(text.to_owned()).color(color).size(13.0))
            .fill(self.button)
            .stroke(Stroke::new(1.0, self.pill_border))
            .corner_radius(CornerRadius::same(255))
    }
}

/// A monospace, uppercase, positively-tracked eyebrow label (the brand's
/// "code comment" voice) — used above every section.
fn eyebrow(text: &str, color: Color32, size: f32) -> RichText {
    RichText::new(text.to_uppercase())
        .family(egui::FontFamily::Monospace)
        .size(size)
        .color(color)
        .strong()
}

/// A display headline: proportional, with tight negative tracking.
fn display(text: &str, size: f32, color: Color32) -> RichText {
    RichText::new(text.to_owned()).size(size).color(color)
}

const ROW_ANIM: Duration = Duration::from_millis(180);
const ACTIVATION_KEY_ANIM: Duration = Duration::from_millis(420);
const SIDEBAR_OPEN_WIDTH: f32 = 201.0;
const SIDEBAR_CLOSED_WIDTH: f32 = 64.0;
const SIDEBAR_ANIM_SECS: f32 = 0.45;

struct DeletingKeywordRow {
    index: usize,
    label: String,
    selected: bool,
    started: Instant,
}

fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn progress_since(started: Instant, duration: Duration) -> f32 {
    (started.elapsed().as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
}

fn alpha_u8(value: u8, alpha: f32) -> u8 {
    ((value as f32) * alpha.clamp(0.0, 1.0)).round() as u8
}

fn alpha_color(color: Color32, alpha: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha_u8(color.a(), alpha))
}

fn lerp_f32(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t.clamp(0.0, 1.0)
}

fn lerp_u8(from: u8, to: u8, t: f32) -> u8 {
    lerp_f32(from as f32, to as f32, t).round() as u8
}

fn mix_color(from: Color32, to: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgba_unmultiplied(
        lerp_u8(from.r(), to.r(), t),
        lerp_u8(from.g(), to.g(), t),
        lerp_u8(from.b(), to.b(), t),
        lerp_u8(from.a(), to.a(), t),
    )
}

fn paint_keyword_row(
    ui: &mut egui::Ui,
    p: &Palette,
    rect: egui::Rect,
    label: &str,
    selected_t: f32,
    hover_t: f32,
    alpha: f32,
    y_offset: f32,
) {
    if ui.is_rect_visible(rect) {
        let text_x = rect.left() + 13.0;
        let text_color = alpha_color(
            mix_color(mix_color(p.muted, p.body, hover_t), p.text, selected_t),
            alpha,
        );
        let selected_fill = if p.dark {
            Color32::from_rgb(0x1E, 0x1E, 0x1E)
        } else {
            Color32::from_rgb(0xED, 0xED, 0xED)
        };
        let hover_fill = if p.dark {
            Color32::from_rgb(0x16, 0x16, 0x16)
        } else {
            Color32::from_rgb(0xF5, 0xF5, 0xF5)
        };

        ui.painter().rect_filled(
            rect,
            CornerRadius::same(9),
            alpha_color(hover_fill, alpha * hover_t * (1.0 - selected_t)),
        );
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(9),
            alpha_color(selected_fill, alpha * selected_t),
        );
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(9),
            Stroke::new(1.0, alpha_color(p.hairline, alpha * selected_t)),
            egui::StrokeKind::Inside,
        );

        ui.painter().text(
            egui::pos2(text_x, rect.center().y + y_offset),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(14.0),
            text_color,
        );
    }
}

fn keyword_row_button(
    ui: &mut egui::Ui,
    p: &Palette,
    label: &str,
    selected: bool,
    width: f32,
    alpha: f32,
    y_offset: f32,
) -> egui::Response {
    let size = Vec2::new(width, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let selected_t = ui
        .ctx()
        .animate_bool_responsive(ui.id().with("row_selected"), selected);
    let hover_t = ui
        .ctx()
        .animate_bool_responsive(ui.id().with("row_hovered"), response.hovered() && !selected);

    paint_keyword_row(
        ui,
        p,
        rect,
        label,
        selected_t,
        hover_t,
        alpha,
        y_offset,
    );

    response
}

fn paint_keyboard_icon(painter: &egui::Painter, rect: egui::Rect, color: Color32) {
    painter.rect_stroke(
        rect,
        CornerRadius::same(3),
        Stroke::new(1.2, color),
        egui::StrokeKind::Inside,
    );
    let key_r = 1.0;
    for (x, y) in [(0.28, 0.38), (0.5, 0.38), (0.72, 0.38)] {
        painter.circle_filled(
            egui::pos2(
                egui::lerp(rect.left()..=rect.right(), x),
                egui::lerp(rect.top()..=rect.bottom(), y),
            ),
            key_r,
            color,
        );
    }
    painter.line_segment(
        [
            egui::pos2(rect.left() + 4.5, rect.bottom() - 4.0),
            egui::pos2(rect.right() - 4.5, rect.bottom() - 4.0),
        ],
        Stroke::new(1.2, color),
    );
}

fn paint_chevron(painter: &egui::Painter, rect: egui::Rect, color: Color32, open_t: f32) {
    let center = rect.center();
    let side_y = center.y - 2.0 + 4.0 * open_t;
    let tip_y = center.y + 2.0 - 4.0 * open_t;
    painter.line_segment(
        [egui::pos2(center.x - 4.0, side_y), egui::pos2(center.x, tip_y)],
        Stroke::new(1.4, color),
    );
    painter.line_segment(
        [egui::pos2(center.x, tip_y), egui::pos2(center.x + 4.0, side_y)],
        Stroke::new(1.4, color),
    );
}

fn paint_staggered_text(
    ui: &egui::Ui,
    rect: egui::Rect,
    text: &str,
    font_id: egui::FontId,
    color: Color32,
    started: Instant,
) {
    let elapsed = started.elapsed().as_secs_f32();
    if elapsed < ACTIVATION_KEY_ANIM.as_secs_f32() + 0.16 {
        ui.ctx().request_repaint();
    }

    let make_galleys = |font_id: &egui::FontId| -> Vec<_> {
        text.chars()
            .map(|ch| ui.painter().layout_no_wrap(ch.to_string(), font_id.clone(), color))
            .collect()
    };
    let mut font_id = font_id;
    let mut galleys = make_galleys(&font_id);
    let mut total_w: f32 = galleys.iter().map(|galley| galley.size().x).sum();
    if total_w > rect.width() && total_w > 0.0 {
        font_id.size = (font_id.size * rect.width() / total_w).max(7.5);
        galleys = make_galleys(&font_id);
        total_w = galleys.iter().map(|galley| galley.size().x).sum();
    }
    let mut x = rect.center().x - total_w / 2.0;

    for (i, galley) in galleys.into_iter().enumerate() {
        let char_t = ((elapsed - i as f32 * 0.018) / 0.28).clamp(0.0, 1.0);
        let eased = ease_out(char_t);
        let alpha = ease_in_out(char_t);
        let pos = egui::pos2(
            x,
            rect.center().y - galley.size().y / 2.0 + (1.0 - eased) * 14.0,
        );
        ui.painter().galley(pos, galley.clone(), alpha_color(color, alpha));
        x += galley.size().x;
    }
}

fn keyword_row_ghost(
    ui: &mut egui::Ui,
    p: &Palette,
    label: &str,
    selected: bool,
    width: f32,
    height: f32,
    alpha: f32,
) {
    if height <= 0.5 {
        return;
    }

    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::hover());
    let row_rect = egui::Rect::from_min_size(rect.min, Vec2::new(width, 30.0_f32.min(height)));
    paint_keyword_row(
        ui,
        p,
        row_rect,
        label,
        if selected { 1.0 } else { 0.0 },
        0.0,
        alpha,
        -4.0 * (1.0 - alpha),
    );
}

fn trash_button(ui: &mut egui::Ui, p: &Palette, selected: bool, alpha: f32) -> egui::Response {
    let size = Vec2::splat(24.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if response.hovered() {
            alpha_color(p.hover, alpha)
        } else {
            Color32::TRANSPARENT
        };
        let color = if response.hovered() || selected {
            alpha_color(p.text, alpha)
        } else {
            alpha_color(p.muted, alpha)
        };
        let stroke = Stroke::new(1.4, color);
        ui.painter().rect_filled(rect, CornerRadius::same(6), fill);

        let to_pos = |x: f32, y: f32| {
            egui::pos2(
                rect.left() + x / 24.0 * rect.width(),
                rect.top() + y / 24.0 * rect.height(),
            )
        };
        let path = [
            (3.0, 6.0, 21.0, 6.0),
            (8.0, 6.0, 8.0, 4.0),
            (8.0, 4.0, 9.0, 3.0),
            (9.0, 3.0, 15.0, 3.0),
            (15.0, 3.0, 16.0, 4.0),
            (16.0, 4.0, 16.0, 6.0),
            (5.0, 6.0, 5.0, 20.0),
            (5.0, 20.0, 6.0, 21.0),
            (6.0, 21.0, 18.0, 21.0),
            (18.0, 21.0, 19.0, 20.0),
            (19.0, 20.0, 19.0, 6.0),
            (10.0, 11.0, 10.0, 17.0),
            (14.0, 11.0, 14.0, 17.0),
        ];
        for (x1, y1, x2, y2) in path {
            ui.painter()
                .line_segment([to_pos(x1, y1), to_pos(x2, y2)], stroke);
        }
    }
    response.on_hover_text("Delete keyword")
}

fn sidebar_icon_button(
    ui: &mut egui::Ui,
    p: &Palette,
    id: &str,
    title: &str,
    paint: impl FnOnce(&egui::Painter, egui::Rect, Stroke),
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(32.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let hover_t = ui
            .ctx()
            .animate_bool_responsive(ui.id().with((id, "hovered")), response.hovered());
        ui.painter().rect_filled(
            rect,
            CornerRadius::same(9),
            alpha_color(p.hover, hover_t),
        );
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(9),
            Stroke::new(1.0, alpha_color(p.pill_border, 0.35 + hover_t * 0.65)),
            egui::StrokeKind::Inside,
        );
        paint(ui.painter(), rect, Stroke::new(1.5, p.text));
    }
    response.on_hover_text(title)
}

fn plus_icon_button(ui: &mut egui::Ui, p: &Palette) -> egui::Response {
    sidebar_icon_button(ui, p, "plus", "New keyword", |painter, rect, stroke| {
        let c = rect.center();
        painter.line_segment([egui::pos2(c.x - 5.0, c.y), egui::pos2(c.x + 5.0, c.y)], stroke);
        painter.line_segment([egui::pos2(c.x, c.y - 5.0), egui::pos2(c.x, c.y + 5.0)], stroke);
    })
}

fn sidebar_toggle_button(ui: &mut egui::Ui, p: &Palette, open: bool) -> egui::Response {
    let title = if open { "Collapse sidebar" } else { "Expand sidebar" };
    sidebar_icon_button(ui, p, "sidebar_toggle", title, |painter, rect, stroke| {
        let box_rect = egui::Rect::from_center_size(rect.center(), Vec2::new(15.0, 13.0));
        painter.rect_stroke(
            box_rect,
            CornerRadius::same(3),
            stroke,
            egui::StrokeKind::Inside,
        );
        let divider_x = if open {
            box_rect.left() + 5.0
        } else {
            box_rect.right() - 5.0
        };
        painter.line_segment(
            [
                egui::pos2(divider_x, box_rect.top() + 1.0),
                egui::pos2(divider_x, box_rect.bottom() - 1.0),
            ],
            stroke,
        );
    })
}

fn empty_state_icon(ui: &mut egui::Ui, p: &Palette) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(11), p.card);
    ui.painter().rect_stroke(
        rect,
        CornerRadius::same(11),
        Stroke::new(1.0, p.hairline),
        egui::StrokeKind::Inside,
    );

    let stroke = Stroke::new(1.5, p.muted);
    let x = rect.left() + 12.0;
    let y = rect.top() + 14.0;
    for (n, w) in [20.0, 14.0, 10.0].into_iter().enumerate() {
        let line_y = y + (n as f32 * 7.0);
        ui.painter()
            .line_segment([egui::pos2(x, line_y), egui::pos2(x + w, line_y)], stroke);
    }
}

fn apply_input_cursor_style(ui: &mut egui::Ui, p: &Palette) {
    let visuals = ui.visuals_mut();
    visuals.text_cursor.stroke = Stroke::new(4.0, p.text);
    visuals.selection.stroke = Stroke::new(1.0, p.text);
}

const WINDOWS_CAPTURE_KEYS: &[(usize, &str)] = &[
    (0x08, "Backspace"),
    (0x09, "Tab"),
    (0x0D, "Return"),
    (0x13, "Pause"),
    (0x14, "CapsLock"),
    (0x1B, "Escape"),
    (0x20, "Space"),
    (0x21, "PageUp"),
    (0x22, "PageDown"),
    (0x23, "End"),
    (0x24, "Home"),
    (0x25, "LeftArrow"),
    (0x26, "UpArrow"),
    (0x27, "RightArrow"),
    (0x28, "DownArrow"),
    (0x2C, "PrintScreen"),
    (0x2D, "Insert"),
    (0x2E, "Delete"),
    (0x5B, "MetaLeft"),
    (0x5C, "MetaRight"),
    (0x60, "Kp0"),
    (0x61, "Kp1"),
    (0x62, "Kp2"),
    (0x63, "Kp3"),
    (0x64, "Kp4"),
    (0x65, "Kp5"),
    (0x66, "Kp6"),
    (0x67, "Kp7"),
    (0x68, "Kp8"),
    (0x69, "Kp9"),
    (0x6A, "KpMultiply"),
    (0x6B, "KpPlus"),
    (0x6D, "KpMinus"),
    (0x6E, "KpDelete"),
    (0x6F, "KpDivide"),
    (0x70, "F1"),
    (0x71, "F2"),
    (0x72, "F3"),
    (0x73, "F4"),
    (0x74, "F5"),
    (0x75, "F6"),
    (0x76, "F7"),
    (0x77, "F8"),
    (0x78, "F9"),
    (0x79, "F10"),
    (0x7A, "F11"),
    (0x7B, "F12"),
    (0x90, "NumLock"),
    (0x91, "ScrollLock"),
    (0xA0, "ShiftLeft"),
    (0xA1, "ShiftRight"),
    (0xA2, "ControlLeft"),
    (0xA3, "ControlRight"),
    (0xA4, "Alt"),
    (0xA5, "AltGr"),
    (0xBA, "SemiColon"),
    (0xBB, "Equal"),
    (0xBC, "Comma"),
    (0xBD, "Minus"),
    (0xBE, "Dot"),
    (0xBF, "Slash"),
    (0xC0, "BackQuote"),
    (0xDB, "LeftBracket"),
    (0xDC, "BackSlash"),
    (0xDD, "RightBracket"),
    (0xDE, "Quote"),
    (0xE2, "IntlBackslash"),
    (0x30, "Num0"),
    (0x31, "Num1"),
    (0x32, "Num2"),
    (0x33, "Num3"),
    (0x34, "Num4"),
    (0x35, "Num5"),
    (0x36, "Num6"),
    (0x37, "Num7"),
    (0x38, "Num8"),
    (0x39, "Num9"),
    (0x41, "KeyA"),
    (0x42, "KeyB"),
    (0x43, "KeyC"),
    (0x44, "KeyD"),
    (0x45, "KeyE"),
    (0x46, "KeyF"),
    (0x47, "KeyG"),
    (0x48, "KeyH"),
    (0x49, "KeyI"),
    (0x4A, "KeyJ"),
    (0x4B, "KeyK"),
    (0x4C, "KeyL"),
    (0x4D, "KeyM"),
    (0x4E, "KeyN"),
    (0x4F, "KeyO"),
    (0x50, "KeyP"),
    (0x51, "KeyQ"),
    (0x52, "KeyR"),
    (0x53, "KeyS"),
    (0x54, "KeyT"),
    (0x55, "KeyU"),
    (0x56, "KeyV"),
    (0x57, "KeyW"),
    (0x58, "KeyX"),
    (0x59, "KeyY"),
    (0x5A, "KeyZ"),
];

#[cfg(target_os = "windows")]
fn foreground_belongs_to_current_process() -> bool {
    use std::ffi::c_void;

    type Hwnd = *mut c_void;

    unsafe extern "system" {
        fn GetForegroundWindow() -> Hwnd;
        fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
    }

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.is_null() {
        return false;
    }

    let mut foreground_pid = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, &mut foreground_pid);
    }
    foreground_pid == std::process::id()
}

#[cfg(target_os = "windows")]
fn current_windows_key_state() -> [bool; 256] {
    unsafe extern "system" {
        fn GetAsyncKeyState(v_key: i32) -> i16;
    }

    let mut state = [false; 256];
    for (vk, _) in WINDOWS_CAPTURE_KEYS {
        state[*vk] = unsafe { GetAsyncKeyState(*vk as i32) } < 0;
    }
    state
}

#[cfg(not(target_os = "windows"))]
fn current_windows_key_state() -> [bool; 256] {
    [false; 256]
}

#[cfg(target_os = "windows")]
fn capture_key_from_windows(previous: &mut [bool; 256]) -> Option<CaptureResult> {
    let current = current_windows_key_state();

    if !foreground_belongs_to_current_process() {
        *previous = current;
        return None;
    }

    let result = WINDOWS_CAPTURE_KEYS
        .iter()
        .find(|(vk, _)| current[*vk] && !previous[*vk])
        .map(|(_, name)| {
            if *name == "Escape" {
                CaptureResult::Cancelled
            } else {
                CaptureResult::Key(ActivationKey::Custom((*name).to_string()))
            }
        });

    *previous = current;
    result
}

#[cfg(not(target_os = "windows"))]
fn capture_key_from_windows(_previous: &mut [bool; 256]) -> Option<CaptureResult> {
    None
}

/// Apply the xAI look for the given palette: single near-black canvas, pill
/// buttons, 8px cards, hairline borders, no shadows.
fn apply_theme(ctx: &egui::Context, p: &Palette) {
    use egui::epaint::Shadow;
    use egui::FontFamily::{Monospace, Proportional};
    use egui::{FontId, TextStyle};

    let egui_theme = if p.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    let mut style = (*ctx.style_of(egui_theme)).clone();

    style
        .text_styles
        .insert(TextStyle::Heading, FontId::new(18.0, Proportional));
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(14.0, Proportional));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::new(13.0, Proportional));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::new(12.0, Proportional));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::new(12.0, Monospace));

    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(14.0, 7.0);
    style.spacing.interact_size.y = 30.0;

    let sm = CornerRadius::same(8); // the brand --radius for inputs / cards
    let v = &mut style.visuals;
    v.dark_mode = p.dark;
    v.override_text_color = Some(p.text);
    v.panel_fill = p.canvas;
    v.window_fill = p.canvas;
    v.extreme_bg_color = p.card; // text-edit backgrounds
    v.faint_bg_color = p.faint;
    v.window_stroke = Stroke::new(1.0, p.canvas);
    v.window_corner_radius = sm;
    v.hyperlink_color = p.text;
    v.selection.bg_fill = p.selected;
    v.selection.stroke = Stroke::new(1.0, p.text);
    v.text_cursor.stroke = Stroke::new(4.0, p.text);
    // No shadows — hairline borders carry every elevation cue.
    v.popup_shadow = Shadow::NONE;
    v.window_shadow = Shadow::NONE;

    let w = &mut v.widgets;
    for ws in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        ws.corner_radius = sm;
        ws.fg_stroke = Stroke::new(1.0, p.text);
        ws.expansion = 0.0;
    }
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.canvas);
    w.inactive.bg_fill = p.card;
    w.inactive.weak_bg_fill = p.card;
    w.inactive.bg_stroke = Stroke::new(1.0, p.hairline);
    w.hovered.bg_fill = p.hover;
    w.hovered.weak_bg_fill = p.hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.focus);
    w.active.bg_fill = p.pressed;
    w.active.weak_bg_fill = p.pressed;
    w.active.bg_stroke = Stroke::new(1.0, p.focus);
    w.open.bg_fill = p.hover;
    w.open.weak_bg_fill = p.hover;
    w.open.bg_stroke = Stroke::new(1.0, p.focus);

    ctx.set_style_of(egui_theme, style.clone());
    ctx.set_theme(egui_theme);
    ctx.set_global_style(style);
}

/// The in-app keyword manager. Owns the editable list of entries, the live
/// engine config (shared with the keyboard hook), the tray icon, and the
/// hide-to-tray / quit signalling.
pub struct EditorApp {
    entries: Vec<MatchEntry>,
    entry_births: Vec<Instant>,
    entry_ids: Vec<u64>,
    next_entry_id: u64,
    deleting_rows: Vec<DeletingKeywordRow>,
    activation: ActivationKey,
    activation_anim_started: Instant,
    theme: Theme,
    selected: Option<usize>,
    sidebar_open: bool,
    shared: Arc<Mutex<Config>>,
    // Custom-key capture popup state: `capturing` shows the modal; `captured`
    // holds the key once pressed (None while still waiting for a key).
    capturing: bool,
    captured: Option<ActivationKey>,
    capture_prev_keys: [bool; 256],
    path: PathBuf,
    status: String,
    quitting: Arc<AtomicBool>,
    // Tray icon must stay alive for the program's lifetime.
    _tray: TrayIcon,
}

impl EditorApp {
    /// Build the app: load entries from `path`, create the tray icon, and spawn
    /// a thread that turns tray-menu clicks into viewport commands (show / quit).
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        shared: Arc<Mutex<Config>>,
        path: PathBuf,
        initial_text: &str,
    ) -> Self {
        let fd = parse_file(initial_text).unwrap_or_default();
        let (activation, theme, entries) = (fd.activation, fd.theme, fd.entries);
        let entry_count = entries.len();
        let now = Instant::now();
        let settled = now.checked_sub(ROW_ANIM).unwrap_or(now);

        apply_theme(&cc.egui_ctx, &Palette::for_theme(theme));

        // --- tray icon + menu ---
        let menu = Menu::new();
        let open = MenuItem::new("Open editor", true, None);
        let quit = MenuItem::new("Quit", true, None);
        menu.append(&open).unwrap();
        menu.append(&quit).unwrap();
        let open_id = open.id().clone();
        let quit_id = quit.id().clone();

        let tray = TrayIconBuilder::new()
            .with_tooltip("TextSwitch")
            .with_menu(Box::new(menu))
            .with_icon(tray_icon())
            .build()
            .expect("failed to build tray icon");

        let quitting = Arc::new(AtomicBool::new(false));

        // Menu-event thread: wakes the (possibly hidden) window via the egui
        // context, which can send viewport commands from any thread.
        let ctx = cc.egui_ctx.clone();
        let quitting_thread = quitting.clone();
        std::thread::spawn(move || {
            let rx = tray_icon::menu::MenuEvent::receiver();
            while let Ok(ev) = rx.recv() {
                if ev.id == open_id {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else if ev.id == quit_id {
                    quitting_thread.store(true, Ordering::SeqCst);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        });

        Self {
            entries,
            entry_births: vec![settled; entry_count],
            entry_ids: (0..entry_count as u64).collect(),
            next_entry_id: entry_count as u64,
            deleting_rows: Vec::new(),
            activation,
            activation_anim_started: settled,
            theme,
            selected: None,
            sidebar_open: true,
            shared,
            capturing: false,
            captured: None,
            capture_prev_keys: current_windows_key_state(),
            path,
            status: String::new(),
            quitting,
            _tray: tray,
        }
    }

    /// Write entries + settings to disk and update the live engine config.
    fn save(&mut self) {
        let text = entries_to_toml(&self.activation, self.theme, &self.entries);
        match std::fs::write(&self.path, &text) {
            Ok(()) => {
                self.sync_shared_config();
                self.status = "Saved".to_string();
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn sync_shared_config(&self) {
        let mut cfg = entries_to_config(&self.entries);
        cfg.activation = self.activation.clone();
        cfg.theme = self.theme;
        *self.shared.lock().unwrap() = cfg;
    }

    fn apply_activation(&mut self, key: ActivationKey, mark_unsaved: bool) {
        if self.activation == key {
            return;
        }

        self.shared.lock().unwrap().activation = key.clone();
        self.activation = key;
        self.activation_anim_started = Instant::now();
        if mark_unsaved {
            self.status = "Unsaved".to_string();
        }
    }

    fn add_keyword(&mut self) {
        self.entries.push(MatchEntry {
            trigger: String::new(),
            replace: String::new(),
        });
        self.entry_births.push(Instant::now());
        self.entry_ids.push(self.next_entry_id);
        self.next_entry_id += 1;
        self.selected = Some(self.entries.len() - 1);
        self.sync_shared_config();
        self.status = "Unsaved".to_string();
    }

    fn delete_keyword(&mut self, i: usize) {
        if i >= self.entries.len() {
            return;
        }

        let label = if self.entries[i].trigger.is_empty() {
            "Untitled".to_string()
        } else {
            self.entries[i].trigger.clone()
        };
        self.deleting_rows.push(DeletingKeywordRow {
            index: i,
            label,
            selected: self.selected == Some(i),
            started: Instant::now(),
        });
        for row in &mut self.deleting_rows {
            if row.index > i {
                row.index -= 1;
            }
        }
        self.deleting_rows.sort_by_key(|row| row.index);

        self.entries.remove(i);
        self.entry_births.remove(i);
        self.entry_ids.remove(i);
        self.selected = match self.selected {
            Some(selected) if selected == i && self.entries.is_empty() => None,
            Some(selected) if selected == i => Some(i.min(self.entries.len() - 1)),
            Some(selected) if selected > i => Some(selected - 1),
            selected => selected,
        };
        self.sync_shared_config();
        self.status = "Unsaved".to_string();
    }

    fn sanitize_trigger(&mut self, i: usize) {
        let clean: String = self.entries[i]
            .trigger
            .chars()
            .filter_map(|c| {
                let c = c.to_ascii_lowercase();
                c.is_ascii_alphanumeric().then_some(c)
            })
            .collect();
        if clean != self.entries[i].trigger {
            self.entries[i].trigger = clean;
        }
    }

    /// Open the popup that captures the next key press from the editor window.
    fn arm_capture(&mut self) {
        self.capturing = true;
        self.captured = None;
        self.capture_prev_keys = current_windows_key_state();
    }

    fn activation_picker(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let id = ui.id().with("activation_key_button");
        let popup_id = id.with("popup");
        let size = Vec2::new(150.0, 34.0);
        let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
        let response = ui.interact(rect, id, egui::Sense::click());
        let open_now = egui::Popup::is_id_open(ui.ctx(), popup_id) || response.clicked();
        let press_t = ui
            .ctx()
            .animate_bool_responsive(id.with("press"), response.is_pointer_button_down_on());
        let open_t = ui.ctx().animate_bool_responsive(id.with("open"), open_now);
        let hover_t = ui
            .ctx()
            .animate_bool_responsive(id.with("hover"), response.hovered() || open_now);
        let flash_t = 1.0 - progress_since(self.activation_anim_started, ACTIVATION_KEY_ANIM);
        let draw_rect = egui::Rect::from_center_size(rect.center(), rect.size() * (1.0 - press_t * 0.025));
        let fill = mix_color(mix_color(p.button, p.hover, hover_t), p.pressed, press_t);
        let stroke = mix_color(p.pill_border, p.focus, hover_t.max(open_t));

        ui.painter()
            .rect_filled(draw_rect, CornerRadius::same(255), fill);
        ui.painter().rect_stroke(
            draw_rect,
            CornerRadius::same(255),
            Stroke::new(1.0 + flash_t * 0.6, mix_color(stroke, p.accent, flash_t * 0.75)),
            egui::StrokeKind::Inside,
        );

        let icon_color = mix_color(p.muted, p.text, hover_t.max(open_t) * 0.6);
        paint_keyboard_icon(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(draw_rect.left() + 18.0, draw_rect.center().y),
                Vec2::new(16.0, 12.0),
            ),
            icon_color,
        );
        paint_chevron(
            ui.painter(),
            egui::Rect::from_center_size(
                egui::pos2(draw_rect.right() - 17.0, draw_rect.center().y),
                Vec2::splat(12.0),
            ),
            icon_color,
            open_t,
        );
        paint_staggered_text(
            ui,
            egui::Rect::from_min_max(
                egui::pos2(draw_rect.left() + 34.0, draw_rect.top()),
                egui::pos2(draw_rect.right() - 30.0, draw_rect.bottom()),
            ),
            &self.activation.label(),
            egui::FontId::monospace(11.5),
            p.body,
            self.activation_anim_started,
        );

        egui::Popup::menu(&response)
            .id(popup_id)
            .width(rect.width())
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .frame(
                egui::Frame::new()
                    .fill(p.card)
                    .stroke(Stroke::new(1.0, p.hairline))
                    .corner_radius(CornerRadius::same(13))
                    .inner_margin(Margin::same(5)),
            )
            .show(|ui| {
                ui.set_min_width(rect.width() - 10.0);
                for k in ActivationKey::ALL {
                    if ui
                        .selectable_label(self.activation == k, k.label())
                        .clicked()
                    {
                        self.apply_activation(k, true);
                        egui::Popup::close_id(ui.ctx(), popup_id);
                    }
                }
                ui.separator();
                if ui.selectable_label(false, "Custom - press a key...").clicked() {
                    self.arm_capture();
                    egui::Popup::close_id(ui.ctx(), popup_id);
                }
            });
    }

    /// While capturing, poll the hook for a pressed key and draw a modal popup
    /// that prompts for a key and then shows the captured key's name. The key
    /// itself is swallowed by the hook, so we drive repaints ourselves.
    fn capture_popup(&mut self, ui: &mut egui::Ui, p: &Palette) {
        if !self.capturing {
            return;
        }

        // While waiting for a key, repaint every frame so the hook-captured key
        // appears instantly; once captured, the popup is static, so a slow tick
        // is enough to keep it responsive without spinning the CPU.
        if self.captured.is_none() {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }

        let result = capture_key_from_windows(&mut self.capture_prev_keys);
        if let Some(result) = result {
            match result {
                CaptureResult::Key(key) => {
                    // Apply live immediately; popup shows the name, Save persists.
                    self.apply_activation(key.clone(), false);
                    self.captured = Some(key);
                }
                CaptureResult::Cancelled => {
                    self.capturing = false;
                    self.status = "Capture cancelled.".to_string();
                    return;
                }
            }
        }

        let screen = ui.ctx().content_rect();
        ui.ctx()
            .layer_painter(egui::LayerId::new(
                egui::Order::Middle,
                egui::Id::new("capture_dim"),
            ))
            .rect_filled(screen, CornerRadius::ZERO, Color32::from_black_alpha(158));

        let captured = self.captured.clone();
        let mut close = false;
        let mut rearm = false;

        egui::Window::new("capture")
            .id(egui::Id::new("capture_window"))
            .order(egui::Order::Foreground)
            .title_bar(false)
            .resizable(false)
            .fixed_size(Vec2::new(278.0, 128.0))
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .frame(
                egui::Frame::new()
                    .fill(p.card)
                    .inner_margin(Margin::symmetric(26, 24))
                    .corner_radius(CornerRadius::same(16))
                    .stroke(Stroke::new(1.0, p.hairline)),
            )
            .show(ui.ctx(), |ui| {
                ui.set_width(278.0);
                ui.vertical_centered(|ui| match &captured {
                    None => {
                        ui.label(eyebrow("Listening", p.muted, 11.0));
                        ui.add_space(16.0);
                        ui.label(display("Press any key", 20.0, p.muted));
                        ui.add_space(24.0);
                        ui.horizontal_centered(|ui| {
                            if ui
                                .add(
                                    p.outline_pill("Cancel", p.text)
                                        .min_size(Vec2::new(0.0, 34.0)),
                                )
                                .clicked()
                            {
                                close = true;
                            }
                            ui.add_enabled(
                                false,
                                p.primary_pill("Done").min_size(Vec2::new(0.0, 34.0)),
                            );
                        });
                    }
                    Some(key) => {
                        ui.label(eyebrow("Captured", p.muted, 11.0));
                        ui.add_space(16.0);
                        let (key_rect, _) = ui.allocate_exact_size(
                            Vec2::new(ui.available_width(), 34.0),
                            egui::Sense::hover(),
                        );
                        paint_staggered_text(
                            ui,
                            key_rect,
                            &key.label(),
                            egui::FontId::proportional(27.0),
                            p.text,
                            self.activation_anim_started,
                        );
                        ui.add_space(24.0);
                        ui.horizontal_centered(|ui| {
                            if ui
                                .add(
                                    p.outline_pill("Press again", p.text)
                                        .min_size(Vec2::new(0.0, 34.0)),
                                )
                                .clicked()
                            {
                                rearm = true;
                            }
                            if ui
                                .add(p.primary_pill("Done").min_size(Vec2::new(0.0, 34.0)))
                                .clicked()
                            {
                                close = true;
                            }
                        });
                    }
                });
            });

        if rearm {
            self.arm_capture();
        } else if close {
            self.capturing = false;
            if self.captured.is_some() {
                self.status = "Unsaved".to_string();
            }
        }
    }
}

impl eframe::App for EditorApp {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Hide-to-tray: intercept the window close button unless we are quitting.
        // `logic` runs even while the window is hidden, so this stays responsive.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let p = Palette::for_theme(self.theme);
        apply_input_cursor_style(ui, &p);

        self.capture_popup(ui, &p);
        let rows_animating = self
            .entry_births
            .iter()
            .any(|started| progress_since(*started, ROW_ANIM) < 1.0)
            || self
                .deleting_rows
                .iter()
                .any(|row| progress_since(row.started, ROW_ANIM) < 1.0);
        self.deleting_rows
            .retain(|row| progress_since(row.started, ROW_ANIM) < 1.0);
        if rows_animating {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }

        // --- Sidebar: keyword list ---
        let sidebar_t = ui.ctx().animate_bool_with_time_and_easing(
            egui::Id::new("keyword_sidebar_open"),
            self.sidebar_open,
            SIDEBAR_ANIM_SECS,
            egui::emath::easing::cubic_out,
        );
        let sidebar_w = lerp_f32(SIDEBAR_CLOSED_WIDTH, SIDEBAR_OPEN_WIDTH, sidebar_t);
        let sidebar_content_alpha = sidebar_t.clamp(0.0, 1.0);

        egui::Panel::left("keyword_list")
            .resizable(false)
            .default_size(sidebar_w)
            .size_range(egui::Rangef::new(sidebar_w, sidebar_w))
            .show_separator_line(false)
            .frame(
                egui::Frame::new()
                    .fill(p.sidebar)
                    .inner_margin(Margin::symmetric(16, 18)),
            )
            .show_inside(ui, |ui| {
                ui.add_space(2.0);
                if sidebar_content_alpha < 0.12 {
                    ui.vertical_centered(|ui| {
                        if sidebar_toggle_button(ui, &p, self.sidebar_open).clicked() {
                            self.sidebar_open = !self.sidebar_open;
                        }
                        ui.add_space(12.0);
                        if plus_icon_button(ui, &p).clicked() {
                            self.add_keyword();
                        }
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if sidebar_toggle_button(ui, &p, self.sidebar_open).clicked() {
                                self.sidebar_open = !self.sidebar_open;
                            }
                            let toggle = egui::Button::new(eyebrow(
                                self.theme.label(),
                                alpha_color(p.body, sidebar_content_alpha),
                                9.5,
                            ))
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(
                                1.0,
                                alpha_color(p.pill_border, sidebar_content_alpha),
                            ))
                            .corner_radius(CornerRadius::same(255))
                            .min_size(Vec2::new(0.0, 24.0));
                            if ui
                                .add(toggle)
                                .on_hover_text("Switch light / dark theme")
                                .clicked()
                            {
                                self.theme = self.theme.toggled();
                                apply_theme(ui.ctx(), &Palette::for_theme(self.theme));
                            }
                            ui.add(
                                egui::Label::new(eyebrow(
                                    "Keywords",
                                    alpha_color(p.muted, sidebar_content_alpha),
                                    11.0,
                                ))
                                .truncate(),
                            );
                        });
                    });
                    ui.add_space(18.0);

                    if ui
                        .add_sized(
                            [ui.available_width(), 34.0],
                            p.outline_pill(
                                "+  New keyword",
                                alpha_color(p.text, sidebar_content_alpha),
                            )
                            .wrap_mode(egui::TextWrapMode::Extend),
                        )
                        .clicked()
                    {
                        self.add_keyword();
                    }
                    ui.add_space(14.0);

                    let mut delete_index = None;
                    let list_w = ui.available_width().max(32.0);
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_width(list_w);
                            if self.entries.is_empty() {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new("No keywords yet.")
                                        .size(13.0)
                                        .color(alpha_color(p.muted, sidebar_content_alpha)),
                                );
                            }

                            let mut ghost_i = 0;
                            for i in 0..=self.entries.len() {
                                while ghost_i < self.deleting_rows.len()
                                    && self.deleting_rows[ghost_i].index == i
                                {
                                    let row = &self.deleting_rows[ghost_i];
                                    let out = ease_out(progress_since(row.started, ROW_ANIM));
                                    keyword_row_ghost(
                                        ui,
                                        &p,
                                        &row.label,
                                        row.selected,
                                        list_w,
                                        32.0 * (1.0 - out),
                                        (1.0 - out) * sidebar_content_alpha,
                                    );
                                    ui.add_space(2.0 * (1.0 - out));
                                    ghost_i += 1;
                                }

                                if i == self.entries.len() {
                                    break;
                                }

                                let label = if self.entries[i].trigger.is_empty() {
                                    "Untitled".to_string()
                                } else {
                                    self.entries[i].trigger.clone()
                                };
                                let selected = self.selected == Some(i);
                                let row_id = self.entry_ids[i];
                                let appear =
                                    ease_out(progress_since(self.entry_births[i], ROW_ANIM));

                                ui.push_id(row_id, |ui| {
                                    ui.allocate_ui_with_layout(
                                        Vec2::new(list_w, 32.0),
                                        egui::Layout::left_to_right(egui::Align::Center),
                                        |ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;
                                            let delete_w = 24.0;
                                            let gap = ui.spacing().item_spacing.x;
                                            let label_w = (list_w - delete_w - gap).max(48.0);
                                            if keyword_row_button(
                                                ui,
                                                &p,
                                                &label,
                                                selected,
                                                label_w,
                                                appear * sidebar_content_alpha,
                                                (1.0 - appear) * 6.0,
                                            )
                                            .clicked()
                                            {
                                                self.selected = Some(i);
                                            }

                                            if trash_button(
                                                ui,
                                                &p,
                                                selected,
                                                sidebar_content_alpha,
                                            )
                                            .clicked()
                                            {
                                                delete_index = Some(i);
                                            }
                                        },
                                    );
                                });
                                ui.add_space(2.0);
                            }
                        });

                    if let Some(i) = delete_index {
                        self.delete_keyword(i);
                    }
                }
            });

        // --- Editor ---
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(p.canvas)
                    .inner_margin(Margin::same(0)),
            )
            .show_inside(ui, |ui| {
                apply_input_cursor_style(ui, &p);
                let full_rect = ui.max_rect();
                let footer_h = 60.0;
                let footer_top = full_rect.bottom() - footer_h;
                let editor_rect = egui::Rect::from_min_max(
                    full_rect.min,
                    egui::pos2(full_rect.right(), footer_top),
                );
                let footer_rect = egui::Rect::from_min_max(
                    egui::pos2(full_rect.left(), footer_top),
                    full_rect.max,
                );

                ui.allocate_ui_at_rect(editor_rect, |ui| {
                    egui::Frame::new()
                        .fill(p.canvas)
                        .inner_margin(Margin::symmetric(32, 24))
                        .show(ui, |ui| match self.selected {
                            Some(i) if i < self.entries.len() => {
                                egui::ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(eyebrow("Keyword", p.muted, 11.0));
                                        ui.add_space(9.0);
                                        let trigger_output = ui.add_sized(
                                            [ui.available_width(), 42.0],
                                            egui::TextEdit::singleline(
                                                &mut self.entries[i].trigger,
                                            )
                                            .font(egui::TextStyle::Monospace)
                                            .hint_text("keyword")
                                            .desired_width(f32::INFINITY)
                                            .margin(Margin::symmetric(14, 10)),
                                        );
                                        let trigger_changed = trigger_output.changed();
                                        if trigger_changed {
                                            self.sanitize_trigger(i);
                                        }
                                        ui.add_space(8.0);
                                        ui.label(
                                            RichText::new("Lowercase letters and digits only.")
                                                .size(12.5)
                                                .color(p.muted),
                                        );

                                        ui.add_space(22.0);

                                        ui.label(eyebrow("Replacement", p.muted, 11.0));
                                        ui.add_space(9.0);
                                        let replacement_h = 172.0;
                                        let replacement_output = ui.add_sized(
                                            [ui.available_width(), replacement_h],
                                            egui::TextEdit::multiline(&mut self.entries[i].replace)
                                                .desired_width(f32::INFINITY)
                                                .desired_rows(8)
                                                .margin(Margin::symmetric(15, 13))
                                                .hint_text(
                                                    "The text that will replace your keyword...",
                                                ),
                                        );
                                        let replacement_changed = replacement_output.changed();

                                        if trigger_changed || replacement_changed {
                                            self.sync_shared_config();
                                            self.status = "Unsaved".to_string();
                                        }
                                    });
                            }
                            _ => {
                                let top = ((ui.available_height() - 176.0) * 0.5).max(24.0);
                                ui.add_space(top);
                                ui.vertical_centered(|ui| {
                                    empty_state_icon(ui, &p);
                                    ui.add_space(18.0);
                                    ui.label(
                                        display("Select a keyword to edit", 17.0, p.text).strong(),
                                    );
                                    ui.add_space(6.0);
                                    ui.label(
                                        RichText::new("or create a new one from the sidebar.")
                                            .size(13.5)
                                            .color(p.muted),
                                    );
                                    ui.add_space(22.0);
                                    let hint_w = 398.0_f32.min(ui.available_width());
                                    let hint_h = 38.0;
                                    let hint_rect = egui::Rect::from_min_size(
                                        egui::pos2(
                                            ui.max_rect().center().x - hint_w * 0.5,
                                            ui.cursor().top(),
                                        ),
                                        Vec2::new(hint_w, hint_h),
                                    );
                                    ui.allocate_ui_at_rect(hint_rect, |ui| {
                                        egui::Frame::new()
                                            .fill(p.card)
                                            .stroke(Stroke::new(1.0, p.hairline))
                                            .corner_radius(CornerRadius::same(255))
                                            .inner_margin(Margin::symmetric(15, 8))
                                            .show(ui, |ui| {
                                                ui.set_width((hint_w - 30.0).max(0.0));
                                                ui.horizontal(|ui| {
                                                    ui.label(
                                                        RichText::new(
                                                            "Type a keyword anywhere, then press",
                                                        )
                                                        .size(12.5)
                                                        .color(p.muted),
                                                    );
                                                    egui::Frame::new()
                                                        .fill(p.canvas)
                                                        .stroke(Stroke::new(1.0, p.hairline))
                                                        .corner_radius(CornerRadius::same(5))
                                                        .inner_margin(Margin::symmetric(7, 2))
                                                        .show(ui, |ui| {
                                                            ui.label(
                                                                RichText::new(
                                                                    self.activation.label(),
                                                                )
                                                                .family(egui::FontFamily::Monospace)
                                                                .size(11.0)
                                                                .color(p.body),
                                                            );
                                                        });
                                                    ui.label(
                                                        RichText::new("to expand.")
                                                            .size(12.5)
                                                            .color(p.muted),
                                                    );
                                                });
                                            });
                                    });
                                    ui.add_space(hint_h);
                                });
                            }
                        });
                });

                ui.painter().line_segment(
                    [
                        egui::pos2(footer_rect.left(), footer_rect.top()),
                        egui::pos2(footer_rect.right(), footer_rect.top()),
                    ],
                    Stroke::new(1.0, p.hairline),
                );
                ui.allocate_ui_at_rect(footer_rect, |ui| {
                    egui::Frame::new()
                        .fill(p.canvas)
                        .inner_margin(Margin::symmetric(22, 14))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui
                                    .add(p.primary_pill("Save all").min_size(Vec2::new(0.0, 34.0)))
                                    .clicked()
                                {
                                    self.save();
                                }

                                let status_text = if self.status.is_empty() {
                                    if self.selected.is_some() {
                                        "Saved".to_string()
                                    } else {
                                        "No selection".to_string()
                                    }
                                } else {
                                    self.status.clone()
                                };
                                let status_color = if status_text == "Unsaved" {
                                    p.warning
                                } else if self.selected.is_some() || status_text == "Saved" {
                                    p.success
                                } else {
                                    p.muted
                                };

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        self.activation_picker(ui, &p);
                                        ui.add_space(10.0);
                                        ui.label(eyebrow("Expand", p.muted, 10.0));
                                        ui.add_space(14.0);
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(status_text)
                                                    .size(12.5)
                                                    .color(p.muted),
                                            )
                                            .truncate(),
                                        );
                                        let (dot_rect, _) = ui.allocate_exact_size(
                                            Vec2::splat(7.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            dot_rect.center(),
                                            3.5,
                                            status_color,
                                        );

                                    },
                                );
                            });
                        });
                });
            });
    }
}

pub fn app_icon_data() -> egui::IconData {
    const SIZE: u32 = 128;
    const RGBA: &[u8] = include_bytes!("../assets/logo-app-128.rgba");

    egui::IconData {
        rgba: RGBA.to_vec(),
        width: SIZE,
        height: SIZE,
    }
}

fn tray_icon() -> tray_icon::Icon {
    const SIZE: u32 = 32;
    const RGBA: &[u8] = include_bytes!("../assets/logo-tray-32.rgba");

    tray_icon::Icon::from_rgba(RGBA.to_vec(), SIZE, SIZE).expect("valid icon")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_capture_keys_include_letters_and_tab() {
        assert!(WINDOWS_CAPTURE_KEYS.contains(&(0x41, "KeyA")));
        assert!(WINDOWS_CAPTURE_KEYS.contains(&(0x09, "Tab")));
    }

    #[test]
    fn windows_capture_keys_include_modifier_and_toggle_keys() {
        assert!(WINDOWS_CAPTURE_KEYS.contains(&(0xA2, "ControlLeft")));
        assert!(WINDOWS_CAPTURE_KEYS.contains(&(0xA0, "ShiftLeft")));
        assert!(WINDOWS_CAPTURE_KEYS.contains(&(0x14, "CapsLock")));
    }
}
