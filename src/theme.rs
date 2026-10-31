//! A restrained interface: Inter type, warm ink, and one quiet accent. The map
//! carries the colour. Two palettes, dark and light; every colour the interface
//! uses is read from the one in force.

use std::sync::RwLock;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, ThemePreference, Vec2, Visuals,
};
use serde::{Deserialize, Serialize};

/// Which palette the interface is drawn in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Mode {
    #[default]
    Dark,
    Light,
}

impl Mode {
    pub const ALL: [Mode; 2] = [Mode::Dark, Mode::Light];

    pub fn label(self) -> &'static str {
        match self {
            Self::Dark => "Dark",
            Self::Light => "Light",
        }
    }
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub ink: Color32,
    pub ink_dim: Color32,
    pub ink_faint: Color32,
    pub ink_ghost: Color32,
    pub canvas: Color32,
    pub panel: Color32,
    pub panel_solid: Color32,
    pub raised: Color32,
    pub raised_hover: Color32,
    pub hairline: Color32,
    pub overlay: Color32,
    pub accent: Color32,
    pub danger: Color32,
    pub station: Color32,
    pub station_edge: Color32,
    pub selection: Color32,
}

const DARK: Palette = Palette {
    ink: Color32::from_rgb(243, 241, 236),
    ink_dim: Color32::from_rgb(160, 164, 171),
    ink_faint: Color32::from_rgb(106, 111, 119),
    ink_ghost: Color32::from_rgb(72, 77, 85),
    canvas: Color32::from_rgb(233, 231, 226),
    panel: Color32::from_rgb(18, 20, 23),
    panel_solid: Color32::from_rgb(20, 22, 25),
    raised: Color32::from_rgb(28, 31, 35),
    raised_hover: Color32::from_rgb(36, 39, 44),
    hairline: Color32::from_rgb(42, 46, 51),
    overlay: Color32::from_rgba_premultiplied(16, 18, 21, 238),
    accent: Color32::from_rgb(233, 196, 106),
    danger: Color32::from_rgb(226, 122, 105),
    station: Color32::from_rgb(247, 244, 237),
    station_edge: Color32::from_rgb(13, 14, 16),
    selection: Color32::from_rgb(62, 55, 38),
};

const LIGHT: Palette = Palette {
    ink: Color32::from_rgb(27, 29, 33),
    ink_dim: Color32::from_rgb(93, 97, 105),
    ink_faint: Color32::from_rgb(133, 137, 144),
    ink_ghost: Color32::from_rgb(172, 176, 182),
    canvas: Color32::from_rgb(236, 233, 227),
    panel: Color32::from_rgb(245, 244, 241),
    panel_solid: Color32::from_rgb(250, 249, 246),
    raised: Color32::from_rgb(236, 235, 231),
    raised_hover: Color32::from_rgb(228, 227, 222),
    hairline: Color32::from_rgb(218, 217, 211),
    overlay: Color32::from_rgba_premultiplied(250, 249, 246, 238),
    accent: Color32::from_rgb(191, 143, 40),
    danger: Color32::from_rgb(190, 80, 64),
    station: Color32::from_rgb(250, 249, 245),
    station_edge: Color32::from_rgb(22, 23, 26),
    selection: Color32::from_rgb(238, 226, 197),
};

static LIVE: RwLock<Palette> = RwLock::new(DARK);

/// The palette in force. One theme is live at a time, so everything reads
/// through here rather than carrying a colour around.
fn palette() -> Palette {
    *LIVE.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn ink() -> Color32 {
    palette().ink
}

pub fn ink_dim() -> Color32 {
    palette().ink_dim
}

pub fn ink_faint() -> Color32 {
    palette().ink_faint
}

pub fn ink_ghost() -> Color32 {
    palette().ink_ghost
}

pub fn canvas() -> Color32 {
    palette().canvas
}

pub fn panel() -> Color32 {
    palette().panel
}

pub fn panel_solid() -> Color32 {
    palette().panel_solid
}

pub fn raised() -> Color32 {
    palette().raised
}

pub fn raised_hover() -> Color32 {
    palette().raised_hover
}

pub fn hairline() -> Color32 {
    palette().hairline
}

pub fn overlay() -> Color32 {
    palette().overlay
}

pub fn accent() -> Color32 {
    palette().accent
}

pub fn danger() -> Color32 {
    palette().danger
}

pub fn station() -> Color32 {
    palette().station
}

pub fn station_edge() -> Color32 {
    palette().station_edge
}

pub const MAP_INK: Color32 = Color32::from_rgb(30, 32, 36);
pub const MAP_HALO: Color32 = Color32::from_rgb(250, 249, 245);
/// The same pairing the other way round, for a dark or photographic basemap.
pub const MAP_INK_ON_DARK: Color32 = Color32::from_rgb(246, 245, 241);
pub const MAP_HALO_ON_DARK: Color32 = Color32::from_rgb(14, 15, 18);
/// The paper tone a light basemap shows before its first tiles arrive.
pub const MAP_BACKDROP: Color32 = Color32::from_rgb(242, 239, 233);

pub fn rgb(color: [u8; 3]) -> Color32 {
    Color32::from_rgb(color[0], color[1], color[2])
}

pub fn alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn install(ctx: &egui::Context, mode: Mode) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter-regular".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-400.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-medium".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-500.ttf")).into(),
    );
    fonts.font_data.insert(
        "inter-semibold".to_owned(),
        FontData::from_static(include_bytes!("../assets/fonts/Inter-600.ttf")).into(),
    );

    let fallback = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let with_fallback = |name: &str| {
        let mut family = vec![name.to_owned()];
        family.extend(fallback.iter().cloned());
        family
    };

    fonts
        .families
        .insert(FontFamily::Proportional, with_fallback("inter-regular"));
    fonts.families.insert(
        FontFamily::Name("Inter Medium".into()),
        with_fallback("inter-medium"),
    );
    fonts.families.insert(
        FontFamily::Name("Inter SemiBold".into()),
        with_fallback("inter-semibold"),
    );
    ctx.set_fonts(fonts);

    apply(ctx, mode);
}

/// Put a palette in force: the widgets, the text, and the colours the rest of
/// the app reads while it paints.
pub fn apply(ctx: &egui::Context, mode: Mode) {
    {
        let mut live = LIVE
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *live = match mode {
            Mode::Dark => DARK,
            Mode::Light => LIGHT,
        };
    }
    ctx.set_theme(match mode {
        Mode::Dark => ThemePreference::Dark,
        Mode::Light => ThemePreference::Light,
    });

    ctx.all_styles_mut(|style| {
        style.visuals = visuals(mode);
        style.spacing.item_spacing = Vec2::new(8.0, 6.0);
        style.spacing.button_padding = Vec2::new(11.0, 7.0);
        style.spacing.interact_size = Vec2::new(34.0, 28.0);
        style.spacing.slider_width = 128.0;
        style.spacing.indent = 12.0;
        style.spacing.window_margin = Margin::same(16);
        style.spacing.menu_margin = Margin::same(6);

        use TextStyle::{Body, Button, Heading, Monospace, Small};
        style
            .text_styles
            .insert(Heading, FontId::new(20.0, FontFamily::Proportional));
        style
            .text_styles
            .insert(Body, FontId::new(13.0, FontFamily::Proportional));
        style
            .text_styles
            .insert(Button, FontId::new(12.5, FontFamily::Proportional));
        style
            .text_styles
            .insert(Small, FontId::new(11.0, FontFamily::Proportional));
        style
            .text_styles
            .insert(Monospace, FontId::new(11.0, FontFamily::Monospace));

        style.visuals.widgets.inactive.expansion = 0.0;
        style.visuals.widgets.hovered.expansion = 0.0;
        style.visuals.widgets.active.expansion = 0.0;
    });
}

fn visuals(mode: Mode) -> Visuals {
    let p = palette();
    let mut visuals = match mode {
        Mode::Dark => Visuals::dark(),
        Mode::Light => Visuals::light(),
    };
    visuals.dark_mode = mode == Mode::Dark;
    visuals.override_text_color = Some(p.ink);
    visuals.weak_text_color = Some(p.ink_dim);
    visuals.panel_fill = p.panel;
    visuals.window_fill = p.panel_solid;
    visuals.window_stroke = Stroke::new(1.0, p.hairline);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 12],
        blur: 40,
        spread: 0,
        color: Color32::from_black_alpha(if mode == Mode::Dark { 120 } else { 44 }),
    };
    visuals.popup_shadow = visuals.window_shadow;
    visuals.extreme_bg_color = p.canvas;
    visuals.text_edit_bg_color = Some(p.raised);
    visuals.faint_bg_color = p.raised;
    visuals.code_bg_color = p.canvas;
    visuals.hyperlink_color = p.ink_dim;
    visuals.warn_fg_color = p.accent;
    visuals.error_fg_color = p.danger;
    visuals.selection.bg_fill = p.selection;
    visuals.selection.stroke = Stroke::new(1.0, p.accent);
    visuals.slider_trailing_fill = true;
    visuals.striped = false;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = p.panel;
    widgets.noninteractive.weak_bg_fill = p.panel;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, p.hairline);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, p.ink_dim);
    widgets.noninteractive.corner_radius = CornerRadius::same(8);

    widgets.inactive.bg_fill = p.raised;
    widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
    widgets.inactive.fg_stroke = Stroke::new(1.0, p.ink_dim);
    widgets.inactive.corner_radius = CornerRadius::same(8);

    widgets.hovered.bg_fill = p.raised_hover;
    widgets.hovered.weak_bg_fill = p.raised_hover;
    widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
    widgets.hovered.fg_stroke = Stroke::new(1.0, p.ink);
    widgets.hovered.corner_radius = CornerRadius::same(8);

    widgets.active.bg_fill = p.raised_hover;
    widgets.active.weak_bg_fill = p.raised_hover;
    widgets.active.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
    widgets.active.fg_stroke = Stroke::new(1.0, p.ink);
    widgets.active.corner_radius = CornerRadius::same(8);

    widgets.open.bg_fill = p.raised_hover;
    widgets.open.weak_bg_fill = p.raised_hover;
    widgets.open.bg_stroke = Stroke::new(1.0, p.hairline);
    widgets.open.fg_stroke = Stroke::new(1.0, p.ink);
    widgets.open.corner_radius = CornerRadius::same(8);

    visuals
}

/// Line colours chosen to sit legibly on the basemaps.
pub const PALETTE: [[u8; 3]; 10] = [
    [235, 87, 87],
    [240, 168, 74],
    [233, 196, 106],
    [97, 185, 137],
    [86, 156, 214],
    [129, 122, 214],
    [221, 120, 176],
    [101, 196, 200],
    [241, 240, 235],
    [151, 158, 168],
];
