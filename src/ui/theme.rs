//! Visual design: palette, typography and widget styling for light and dark mode.

use eframe::egui::{
    self, style::Selection, Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Theme,
    ThemePreference, Visuals,
};

use crate::settings::ThemePref;

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub surface: Color32,
    pub surface_hover: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub danger: Color32,
    pub warning: Color32,
}

pub const ACCENT: Color32 = Color32::from_rgb(108, 92, 231);
pub const CORAL: Color32 = Color32::from_rgb(255, 90, 95);

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: Color32::from_rgb(19, 20, 25),
            surface: Color32::from_rgb(29, 31, 38),
            surface_hover: Color32::from_rgb(38, 41, 51),
            border: Color32::from_rgb(45, 48, 59),
            text: Color32::from_rgb(232, 234, 240),
            text_dim: Color32::from_rgb(140, 145, 162),
            accent: Color32::from_rgb(132, 118, 255),
            accent_soft: Color32::from_rgba_unmultiplied(132, 118, 255, 40),
            danger: CORAL,
            warning: Color32::from_rgb(245, 176, 65),
        }
    } else {
        Palette {
            bg: Color32::from_rgb(246, 247, 251),
            surface: Color32::WHITE,
            surface_hover: Color32::from_rgb(240, 241, 248),
            border: Color32::from_rgb(226, 228, 236),
            text: Color32::from_rgb(26, 28, 36),
            text_dim: Color32::from_rgb(105, 110, 128),
            accent: ACCENT,
            accent_soft: Color32::from_rgba_unmultiplied(108, 92, 231, 28),
            danger: Color32::from_rgb(232, 64, 70),
            warning: Color32::from_rgb(196, 128, 20),
        }
    }
}

pub fn current(ctx: &egui::Context) -> Palette {
    palette(ctx.global_style().visuals.dark_mode)
}

fn visuals(dark: bool) -> Visuals {
    let p = palette(dark);
    let mut v = if dark { Visuals::dark() } else { Visuals::light() };
    v.panel_fill = p.bg;
    v.window_fill = p.surface;
    v.extreme_bg_color = if dark { Color32::from_rgb(14, 15, 19) } else { Color32::from_rgb(236, 238, 244) };
    v.faint_bg_color = p.surface;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.accent;
    v.selection = Selection { bg_fill: p.accent_soft, stroke: Stroke::new(1.0, p.accent) };
    v.window_corner_radius = CornerRadius::same(12);
    v.menu_corner_radius = CornerRadius::same(10);
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_shadow = Shadow { offset: [0, 8], blur: 24, spread: 0, color: Color32::from_black_alpha(if dark { 90 } else { 30 }) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(if dark { 80 } else { 25 }) };

    let r = CornerRadius::same(8);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.surface;
    w.noninteractive.weak_bg_fill = p.surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = p.surface;
    w.inactive.weak_bg_fill = p.surface_hover;
    w.inactive.bg_stroke = Stroke::new(1.0, p.border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text);
    w.inactive.corner_radius = r;

    w.hovered.bg_fill = p.surface_hover;
    w.hovered.weak_bg_fill = p.surface_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.accent);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = p.accent_soft;
    w.active.weak_bg_fill = p.accent_soft;
    w.active.bg_stroke = Stroke::new(1.0, p.accent);
    w.active.fg_stroke = Stroke::new(1.0, p.text);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open = w.active;
    v
}

/// The platform UI font (read from the OS at runtime, never bundled) plus a fallback
/// that has the ⌘⌥⇧⌃ key symbols. Each candidate is parsed before use.
fn system_fonts() -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let (ui, symbols): (&[&str], &[&str]) = if cfg!(target_os = "macos") {
        (&["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/HelveticaNeue.ttc"], &["/System/Library/Fonts/Keyboard.ttf", "/System/Library/Fonts/Apple Symbols.ttf"])
    } else if cfg!(windows) {
        (&["C:\\Windows\\Fonts\\segoeui.ttf"], &["C:\\Windows\\Fonts\\seguisym.ttf"])
    } else {
        (
            &[
                "/usr/share/fonts/truetype/ubuntu/Ubuntu[wdth,wght].ttf",
                "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/cantarell/Cantarell-VF.otf",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ],
            &["/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/usr/share/fonts/TTF/DejaVuSans.ttf"],
        )
    };
    let load = |paths: &[&str], must_have: char| {
        paths.iter().find_map(|p| {
            let bytes = std::fs::read(p).ok()?;
            let font = skrifa::FontRef::from_index(&bytes, 0).ok()?;
            use skrifa::MetadataProvider;
            font.charmap().map(must_have)?;
            Some(bytes)
        })
    };
    (load(ui, 'g'), load(symbols, if cfg!(target_os = "macos") { '⌥' } else { '→' }))
}

pub fn install(ctx: &egui::Context, pref: ThemePref) {
    let mut fonts = egui::FontDefinitions::default();
    let (ui_font, symbol_font) = system_fonts();
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    if ui_font.is_some() {
        prop.insert(0, "system-ui".to_owned());
    }
    if symbol_font.is_some() {
        prop.push("system-symbols".to_owned());
    }
    if let Some(bytes) = ui_font {
        fonts.font_data.insert("system-ui".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    }
    if let Some(bytes) = symbol_font {
        fonts.font_data.insert("system-symbols".to_owned(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
    }
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    ctx.set_visuals_of(Theme::Dark, visuals(true));
    ctx.set_visuals_of(Theme::Light, visuals(false));
    for theme in [Theme::Dark, Theme::Light] {
        ctx.style_mut_of(theme, |s| {
            s.text_styles = [
                (TextStyle::Heading, FontId::new(19.0, FontFamily::Proportional)),
                (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
                (TextStyle::Button, FontId::new(14.0, FontFamily::Proportional)),
                (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
                (TextStyle::Monospace, FontId::new(13.0, FontFamily::Monospace)),
            ]
            .into();
            s.spacing.item_spacing = egui::vec2(10.0, 10.0);
            s.spacing.button_padding = egui::vec2(12.0, 6.0);
            s.spacing.interact_size.y = 30.0;
            s.spacing.menu_margin = Margin::same(8);
            s.spacing.combo_width = 150.0;
            s.interaction.selectable_labels = false;
        });
    }
    set_theme(ctx, pref);
}

pub fn set_theme(ctx: &egui::Context, pref: ThemePref) {
    ctx.set_theme(match pref {
        ThemePref::System => ThemePreference::System,
        ThemePref::Dark => ThemePreference::Dark,
        ThemePref::Light => ThemePreference::Light,
    });
}
