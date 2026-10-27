//! A restrained, dark interface: Inter type, warm off-white ink, and one
//! quiet accent. The map carries the colour.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Vec2, Visuals,
};

pub const INK: Color32 = Color32::from_rgb(243, 241, 236);
pub const INK_DIM: Color32 = Color32::from_rgb(160, 164, 171);
pub const INK_FAINT: Color32 = Color32::from_rgb(106, 111, 119);
pub const INK_GHOST: Color32 = Color32::from_rgb(72, 77, 85);

pub const CANVAS: Color32 = Color32::from_rgb(233, 231, 226);
pub const PANEL: Color32 = Color32::from_rgb(18, 20, 23);
pub const PANEL_SOLID: Color32 = Color32::from_rgb(20, 22, 25);
pub const RAISED: Color32 = Color32::from_rgb(28, 31, 35);
pub const RAISED_HOVER: Color32 = Color32::from_rgb(36, 39, 44);
pub const HAIRLINE: Color32 = Color32::from_rgb(42, 46, 51);
pub const OVERLAY: Color32 = Color32::from_rgba_premultiplied(16, 18, 21, 238);

pub const ACCENT: Color32 = Color32::from_rgb(233, 196, 106);
pub const MAP_INK: Color32 = Color32::from_rgb(30, 32, 36);
pub const MAP_HALO: Color32 = Color32::from_rgb(250, 249, 245);
/// The same pairing the other way round, for a dark or photographic basemap.
pub const MAP_INK_ON_DARK: Color32 = Color32::from_rgb(246, 245, 241);
pub const MAP_HALO_ON_DARK: Color32 = Color32::from_rgb(14, 15, 18);
/// The paper tone a light basemap shows before its first tiles arrive.
pub const MAP_BACKDROP: Color32 = Color32::from_rgb(242, 239, 233);
pub const DANGER: Color32 = Color32::from_rgb(226, 122, 105);
pub const STATION: Color32 = Color32::from_rgb(247, 244, 237);
pub const STATION_EDGE: Color32 = Color32::from_rgb(13, 14, 16);

pub fn rgb(color: [u8; 3]) -> Color32 {
    Color32::from_rgb(color[0], color[1], color[2])
}

pub fn alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);

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

    ctx.all_styles_mut(|style| {
        style.visuals = visuals();
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

fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.dark_mode = true;
    visuals.override_text_color = Some(INK);
    visuals.weak_text_color = Some(INK_DIM);
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL_SOLID;
    visuals.window_stroke = Stroke::new(1.0, HAIRLINE);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 12],
        blur: 40,
        spread: 0,
        color: Color32::from_black_alpha(120),
    };
    visuals.popup_shadow = visuals.window_shadow;
    visuals.extreme_bg_color = CANVAS;
    visuals.text_edit_bg_color = Some(RAISED);
    visuals.faint_bg_color = RAISED;
    visuals.code_bg_color = CANVAS;
    visuals.hyperlink_color = INK_DIM;
    visuals.warn_fg_color = ACCENT;
    visuals.error_fg_color = DANGER;
    visuals.selection.bg_fill = Color32::from_rgb(62, 55, 38);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.slider_trailing_fill = true;
    visuals.striped = false;

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = PANEL;
    widgets.noninteractive.weak_bg_fill = PANEL;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, HAIRLINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, INK_DIM);
    widgets.noninteractive.corner_radius = CornerRadius::same(8);

    widgets.inactive.bg_fill = RAISED;
    widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
    widgets.inactive.fg_stroke = Stroke::new(1.0, INK_DIM);
    widgets.inactive.corner_radius = CornerRadius::same(8);

    widgets.hovered.bg_fill = RAISED_HOVER;
    widgets.hovered.weak_bg_fill = RAISED_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
    widgets.hovered.fg_stroke = Stroke::new(1.0, INK);
    widgets.hovered.corner_radius = CornerRadius::same(8);

    widgets.active.bg_fill = Color32::from_rgb(46, 50, 56);
    widgets.active.weak_bg_fill = Color32::from_rgb(46, 50, 56);
    widgets.active.bg_stroke = Stroke::new(1.0, Color32::TRANSPARENT);
    widgets.active.fg_stroke = Stroke::new(1.0, INK);
    widgets.active.corner_radius = CornerRadius::same(8);

    widgets.open.bg_fill = RAISED_HOVER;
    widgets.open.weak_bg_fill = RAISED_HOVER;
    widgets.open.bg_stroke = Stroke::new(1.0, HAIRLINE);
    widgets.open.fg_stroke = Stroke::new(1.0, INK);
    widgets.open.corner_radius = CornerRadius::same(8);

    visuals
}

/// Line colours chosen to sit legibly on the dark basemap.
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
