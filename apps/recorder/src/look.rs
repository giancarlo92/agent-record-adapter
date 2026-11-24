// Part of Agent Record Adapter (see NOTICE).
//! How the bar looks: the colors, sizes and icons of its design, in dark and light, and the small
//! pieces it is drawn with (an icon, a card, a switch).

use eframe::egui::{self, Color32, CornerRadius, Rect, Stroke, StrokeKind, Ui, vec2};

/// The colors of the design, for the theme Windows uses.
#[derive(Clone, Copy)]
pub struct Palette {
    pub card: Color32,
    pub raised: Color32,
    pub border: Color32,
    pub hairline: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub muted: Color32,
    pub accent_bg: Color32,
    pub accent: Color32,
    pub accent_fill: Color32,
    pub on_accent: Color32,
    pub danger: Color32,
    pub hover: Color32,
}

pub const DARK: Palette = Palette {
    card: Color32::from_rgb(0x26, 0x26, 0x24),
    raised: Color32::from_rgb(0x1f, 0x1e, 0x1d),
    border: Color32::from_rgb(0x48, 0x47, 0x43),
    hairline: Color32::from_rgb(0x36, 0x35, 0x32),
    text: Color32::from_rgb(0xf0, 0xee, 0xe6),
    secondary: Color32::from_rgb(0xa8, 0xa6, 0x9d),
    muted: Color32::from_rgb(0x73, 0x72, 0x6c),
    accent_bg: Color32::from_rgb(0x16, 0x2f, 0x4d),
    accent: Color32::from_rgb(0x85, 0xb7, 0xeb),
    accent_fill: Color32::from_rgb(0x37, 0x8a, 0xdd),
    on_accent: Color32::WHITE,
    danger: Color32::from_rgb(0xe2, 0x4b, 0x4a),
    hover: Color32::from_rgb(0x30, 0x30, 0x2e),
};

pub const LIGHT: Palette = Palette {
    card: Color32::WHITE,
    raised: Color32::from_rgb(0xf8, 0xf8, 0xf6),
    border: Color32::from_rgb(0xc8, 0xc6, 0xbe),
    hairline: Color32::from_rgb(0xe6, 0xe4, 0xdd),
    text: Color32::from_rgb(0x14, 0x14, 0x13),
    secondary: Color32::from_rgb(0x5f, 0x5e, 0x5a),
    muted: Color32::from_rgb(0x88, 0x87, 0x80),
    accent_bg: Color32::from_rgb(0xe6, 0xf1, 0xfb),
    accent: Color32::from_rgb(0x18, 0x5f, 0xa5),
    accent_fill: Color32::from_rgb(0x37, 0x8a, 0xdd),
    on_accent: Color32::WHITE,
    danger: Color32::from_rgb(0xe2, 0x4b, 0x4a),
    hover: Color32::from_rgb(0xf1, 0xef, 0xe8),
};

/// The sizes of the design (logical pixels).
pub const BAR_WIDTH: f32 = 230.0;
pub const PANEL_WIDTH: f32 = 250.0;
pub const GAP: f32 = 6.0;
pub const CARD_RADIUS: u8 = 12;
pub const SMALL: f32 = 38.0;
pub const BIG: f32 = 62.0;
pub const ROW: f32 = 39.0;

/// The icons of the design (Tabler, outline, MIT), drawn white and tinted when shown.
pub enum Icon {
    Settings,
    List,
    Close,
    Play,
    Folder,
    Desktop,
    Video,
    Chevron,
    Repair,
    Download,
    Refresh,
}

impl Icon {
    fn svg(&self) -> (&'static str, &'static str) {
        match self {
            Icon::Settings => ("settings", include_str!("../assets/icons/settings.svg")),
            Icon::List => ("list", include_str!("../assets/icons/list.svg")),
            Icon::Close => ("x", include_str!("../assets/icons/x.svg")),
            Icon::Play => (
                "player-play",
                include_str!("../assets/icons/player-play.svg"),
            ),
            Icon::Folder => ("folder", include_str!("../assets/icons/folder.svg")),
            Icon::Desktop => (
                "device-desktop",
                include_str!("../assets/icons/device-desktop.svg"),
            ),
            Icon::Video => ("video", include_str!("../assets/icons/video.svg")),
            Icon::Repair => ("tool", include_str!("../assets/icons/tool.svg")),
            Icon::Download => ("download", include_str!("../assets/icons/download.svg")),
            Icon::Refresh => ("refresh", include_str!("../assets/icons/refresh.svg")),
            Icon::Chevron => (
                "chevron-right",
                include_str!("../assets/icons/chevron-right.svg"),
            ),
        }
    }

    /// Paints the icon, `size` points square, centered in `rect`.
    pub fn paint(&self, ui: &Ui, rect: Rect, size: f32, color: Color32) {
        let (name, svg) = self.svg();
        let white = svg.replace("currentColor", "#ffffff");
        let image =
            egui::Image::from_bytes(format!("bytes://{name}.svg"), white.into_bytes()).tint(color);
        image.paint_at(ui, Rect::from_center_size(rect.center(), vec2(size, size)));
    }
}

/// A card: the rounded, bordered surface the bar and its panels are.
pub fn card(ui: &Ui, rect: Rect, p: &Palette) {
    ui.painter().rect(
        rect,
        CornerRadius::same(CARD_RADIUS),
        p.card,
        Stroke::new(1.0, p.border),
        StrokeKind::Inside,
    );
}

/// The on / off switch of the design.
pub fn switch(ui: &Ui, rect: Rect, on: bool, p: &Palette) {
    let pill = Rect::from_center_size(rect.center(), vec2(34.0, 18.0));
    let (fill, ring) = if on {
        (p.accent_fill, p.accent_fill)
    } else {
        (Color32::TRANSPARENT, p.muted)
    };
    ui.painter().rect(
        pill,
        CornerRadius::same(9),
        fill,
        Stroke::new(1.0, ring),
        StrokeKind::Inside,
    );
    let x = if on {
        pill.left() + 17.0 + 6.0
    } else {
        pill.left() + 3.0 + 6.0
    };
    ui.painter().circle_filled(
        egui::pos2(x, pill.center().y),
        6.0,
        if on { p.on_accent } else { p.secondary },
    );
}

/// The fonts: Segoe UI, the font of Windows, before egui's own.
pub fn fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let windir = std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".to_owned());
    if let Ok(bytes) = std::fs::read(
        std::path::Path::new(&windir)
            .join("Fonts")
            .join("segoeui.ttf"),
    ) {
        fonts
            .font_data
            .insert("segoe".to_owned(), egui::FontData::from_owned(bytes).into());
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe".to_owned());
    }
    if let Ok(bytes) = std::fs::read(
        std::path::Path::new(&windir)
            .join("Fonts")
            .join("consola.ttf"),
    ) {
        fonts.font_data.insert(
            "consolas".to_owned(),
            egui::FontData::from_owned(bytes).into(),
        );
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "consolas".to_owned());
    }
    ctx.set_fonts(fonts);
}
