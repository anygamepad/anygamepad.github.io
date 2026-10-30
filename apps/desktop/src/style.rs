//! The web app's look in egui: the colour tokens of `apps/web/src/style.css`
//! for both themes, its radii and spacing, cards with a border that turns
//! to the accent colour when selected, outlined accent buttons, and the
//! system's UI font, as the web uses `system-ui`.

use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, Stroke, Theme, Visuals};

/// The web's colour tokens (`:root` in `style.css`).
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// `--bg`: the page.
    pub bg: Color32,
    /// `--fg`: text.
    pub fg: Color32,
    /// `--muted`: secondary text.
    pub muted: Color32,
    /// `--card`: cards and fields.
    pub card: Color32,
    /// `--border`: borders and tracks.
    pub border: Color32,
    /// `--accent`: links, selection, lit controls.
    pub accent: Color32,
    /// `--error`.
    pub error: Color32,
}

/// Light theme tokens.
pub const LIGHT: Palette = Palette {
    bg: Color32::from_rgb(0xf7, 0xf7, 0xfb),
    fg: Color32::from_rgb(0x1d, 0x1d, 0x27),
    muted: Color32::from_rgb(0x5d, 0x5d, 0x70),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    border: Color32::from_rgb(0xdc, 0xdc, 0xe6),
    accent: Color32::from_rgb(0x4f, 0x46, 0xe5),
    error: Color32::from_rgb(0xb4, 0x23, 0x18),
};

/// Dark theme tokens.
pub const DARK: Palette = Palette {
    bg: Color32::from_rgb(0x14, 0x14, 0x1c),
    fg: Color32::from_rgb(0xec, 0xec, 0xf4),
    muted: Color32::from_rgb(0xa3, 0xa3, 0xb8),
    card: Color32::from_rgb(0x1e, 0x1e, 0x2a),
    border: Color32::from_rgb(0x33, 0x33, 0x4a),
    accent: Color32::from_rgb(0x8b, 0x85, 0xff),
    error: Color32::from_rgb(0xff, 0x8a, 0x80),
};

/// The palette of the theme `ui` shows.
pub fn palette(ui: &egui::Ui) -> Palette {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

/// `0.375rem` at 16 px, the web's control radius.
const RADIUS: u8 = 6;
/// `0.5rem`, the web's card radius.
const CARD_RADIUS: u8 = 8;

/// Mixes `color` over `base`, `amount` of the way (CSS `color-mix`).
fn mix(color: Color32, base: Color32, amount: f32) -> Color32 {
    let channel =
        |a: u8, b: u8| (f32::from(b) + (f32::from(a) - f32::from(b)) * amount).round() as u8;
    Color32::from_rgb(
        channel(color.r(), base.r()),
        channel(color.g(), base.g()),
        channel(color.b(), base.b()),
    )
}

fn visuals(p: Palette, dark: bool) -> Visuals {
    let mut v = if dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = Stroke::new(1.0_f32, p.border);
    v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.extreme_bg_color = p.card;
    v.text_edit_bg_color = Some(p.card);
    v.faint_bg_color = mix(p.border, p.bg, 0.35);
    v.code_bg_color = mix(p.border, p.card, 0.5);
    v.hyperlink_color = p.accent;
    v.error_fg_color = p.error;
    v.weak_text_color = Some(p.muted);
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0_f32, p.card);

    let w = &mut v.widgets;
    // Labels, separators, frames.
    w.noninteractive.bg_fill = p.bg;
    w.noninteractive.weak_bg_fill = p.bg;
    w.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.fg);
    // Buttons: outlined in the accent colour, as the web's actions.
    w.inactive.bg_fill = p.border;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.inactive.fg_stroke = Stroke::new(1.0_f32, p.accent);
    w.hovered.bg_fill = mix(p.accent, p.card, 0.25);
    w.hovered.weak_bg_fill = mix(p.accent, p.card, 0.12);
    w.hovered.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.5_f32, p.accent);
    w.active.bg_fill = p.accent;
    w.active.weak_bg_fill = mix(p.accent, p.card, 0.25);
    w.active.bg_stroke = Stroke::new(1.0_f32, p.accent);
    w.active.fg_stroke = Stroke::new(1.5_f32, p.fg);
    w.open = w.hovered;
    for widget in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        widget.corner_radius = CornerRadius::same(RADIUS);
        widget.expansion = 0.0;
    }
    v
}

/// A card, as the web's pad list items and boxes: card background, border,
/// accent border when `selected`.
pub fn card(ui: &egui::Ui, selected: bool) -> egui::Frame {
    let p = palette(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(
            if selected { 2.0_f32 } else { 1.0 },
            if selected { p.accent } else { p.border },
        ))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(12)
}

/// A value in `[0, 1]` as the web's `.meter`: a thin rounded track in the
/// border colour, filled with the accent colour from the left.
pub fn meter(ui: &mut egui::Ui, value: f32, width: f32) -> egui::Response {
    let p = palette(ui);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 6.0), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, p.border);
    let mut fill = rect;
    fill.set_width(rect.width() * value.clamp(0.0, 1.0));
    painter.rect_filled(fill, 3.0, p.accent);
    response
}

/// System UI fonts per OS, as the web's `system-ui` resolves them; the first
/// one found becomes the main proportional font.
const SYSTEM_FONTS: [&str; 9] = [
    "C:\\Windows\\Fonts\\segoeui.ttf",
    "/System/Library/Fonts/SFNS.ttf",
    "/System/Library/Fonts/Helvetica.ttc",
    "/usr/share/fonts/truetype/cantarell/Cantarell-VF.otf",
    "/usr/share/fonts/opentype/cantarell/Cantarell-VF.otf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];

/// Installs the web look on `ctx`.
pub fn install(ctx: &egui::Context) {
    ctx.set_visuals_of(Theme::Light, visuals(LIGHT, false));
    ctx.set_visuals_of(Theme::Dark, visuals(DARK, true));
    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(12.0, 5.0);
        style.spacing.interact_size.y = 26.0;
        use egui::{FontFamily, FontId, TextStyle};
        style.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(20.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(14.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(12.5, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(13.0, FontFamily::Monospace),
            ),
        ]
        .into();
    });
    let found = SYSTEM_FONTS
        .iter()
        .find_map(|path| std::fs::read(path).ok().map(|bytes| (*path, bytes)));
    if let Some((path, bytes)) = found {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "system-ui".into(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "system-ui".into());
        }
        ctx.set_fonts(fonts);
        log::info!("UI font: {path}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_like_css() {
        let black = Color32::BLACK;
        let white = Color32::WHITE;
        assert_eq!(mix(white, black, 0.0), black);
        assert_eq!(mix(white, black, 1.0), white);
        assert_eq!(mix(white, black, 0.5), Color32::from_rgb(128, 128, 128));
    }

    #[test]
    fn themes_use_the_web_tokens() {
        let dark = visuals(DARK, true);
        assert_eq!(dark.panel_fill, DARK.bg);
        assert_eq!(dark.selection.bg_fill, DARK.accent);
        assert!(dark.dark_mode);
        let light = visuals(LIGHT, false);
        assert_eq!(light.hyperlink_color, LIGHT.accent);
        assert!(!light.dark_mode);
    }
}
