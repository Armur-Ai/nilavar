//! Matrix-inspired visual identity: near-black surfaces, calm off-white
//! reading text, restrained phosphor green for emphasis and active states.

use egui::epaint::Shadow;
use egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, Style, TextStyle,
    Visuals,
};

pub const BG: Color32 = Color32::from_rgb(0x05, 0x09, 0x08);
pub const PANEL: Color32 = Color32::from_rgb(0x0B, 0x12, 0x10);
pub const PANEL_HI: Color32 = Color32::from_rgb(0x10, 0x1A, 0x17);
pub const BORDER: Color32 = Color32::from_rgb(0x1C, 0x33, 0x2A);
pub const TEXT: Color32 = Color32::from_rgb(0xDC, 0xED, 0xE3);
pub const MUTED: Color32 = Color32::from_rgb(0x8C, 0xA9, 0x9A);
pub const GREEN: Color32 = Color32::from_rgb(0x39, 0xFF, 0x88);
pub const CYAN: Color32 = Color32::from_rgb(0x5D, 0xE4, 0xD4);
pub const AMBER: Color32 = Color32::from_rgb(0xF5, 0xC9, 0x6A);
pub const RED: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x76);

/// Dim green for subtle fills (selection, hover).
pub const GREEN_DIM: Color32 = Color32::from_rgb(0x12, 0x3A, 0x26);
/// Very faint green tint for the active line.
pub const LINE_HI: Color32 = Color32::from_rgb(0x0A, 0x16, 0x12);

pub const MONO: &str = "jetbrains-mono";
pub const MONO_BOLD: &str = "jetbrains-mono-bold";
pub const UI_BOLD: &str = "inter-semibold";

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO.into()))
}

pub fn mono_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(MONO_BOLD.into()))
}

pub fn ui_bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(UI_BOLD.into()))
}

/// Bundled fonts (see `assets/fonts/NOTICE.md` for licenses).
pub fn fonts() -> FontDefinitions {
    let mut defs = FontDefinitions::default();
    let add = |defs: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        defs.font_data.insert(
            name.to_owned(),
            std::sync::Arc::new(FontData::from_static(bytes)),
        );
    };
    add(
        &mut defs,
        "JetBrainsMono-Regular",
        include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
    );
    add(
        &mut defs,
        "JetBrainsMono-Bold",
        include_bytes!("../assets/fonts/JetBrainsMono-Bold.ttf"),
    );
    add(
        &mut defs,
        "Inter-Regular",
        include_bytes!("../assets/fonts/Inter-Regular.ttf"),
    );
    add(
        &mut defs,
        "Inter-SemiBold",
        include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
    );
    add(
        &mut defs,
        "Phosphor",
        include_bytes!("../assets/fonts/Phosphor-Regular.ttf"),
    );

    // Fallback chain keeps egui's bundled fonts (emoji, broad Unicode) last.
    let rest: Vec<String> = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let phosphor = ["Phosphor".to_owned()];

    // Text font first, then Phosphor for icons (the bundled text fonts have
    // their Private Use Area cmap entries removed so they never shadow icons;
    // see assets/fonts/NOTICE.md), then egui's fallbacks.
    let chain = |first: &str| -> Vec<String> {
        let mut v = vec![first.to_owned()];
        v.extend(phosphor.iter().cloned());
        v.extend(rest.iter().cloned());
        v
    };
    defs.families
        .insert(FontFamily::Proportional, chain("Inter-Regular"));
    defs.families
        .insert(FontFamily::Monospace, chain("JetBrainsMono-Regular"));
    defs.families.insert(
        FontFamily::Name(MONO.into()),
        chain("JetBrainsMono-Regular"),
    );
    defs.families.insert(
        FontFamily::Name(MONO_BOLD.into()),
        chain("JetBrainsMono-Bold"),
    );
    defs.families
        .insert(FontFamily::Name(UI_BOLD.into()), chain("Inter-SemiBold"));
    defs
}

/// Applies the palette, spacing and text styles.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    let mut style: Style = (*ctx.global_style()).clone();
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(11.5)),
        (TextStyle::Body, FontId::proportional(14.0)),
        (TextStyle::Button, FontId::proportional(13.5)),
        (TextStyle::Heading, ui_bold(19.0)),
        (TextStyle::Monospace, mono(13.5)),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    style.spacing.window_margin = egui::Margin::same(16);
    style.spacing.menu_margin = egui::Margin::same(8);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;
    style.interaction.tooltip_delay = 0.35;
    style.visuals = visuals();
    ctx.set_global_style(style);
}

fn visuals() -> Visuals {
    let mut v = Visuals::dark();
    let r = CornerRadius::same(5);
    v.dark_mode = true;
    v.override_text_color = None;
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = BG;
    v.faint_bg_color = PANEL_HI;
    v.code_bg_color = Color32::from_rgb(0x0E, 0x18, 0x15);
    v.window_stroke = Stroke::new(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(8);
    v.menu_corner_radius = CornerRadius::same(6);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(160),
    };
    v.popup_shadow = Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };
    v.hyperlink_color = CYAN;
    v.warn_fg_color = AMBER;
    v.error_fg_color = RED;
    v.selection.bg_fill = Color32::from_rgb(0x1A, 0x4D, 0x33);
    v.selection.stroke = Stroke::new(1.0, GREEN);
    v.text_cursor.stroke = Stroke::new(2.0, GREEN);
    v.slider_trailing_fill = true;

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = PANEL;
    w.noninteractive.weak_bg_fill = PANEL;
    w.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
    w.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = Color32::from_rgb(0x16, 0x2A, 0x23);
    w.inactive.weak_bg_fill = PANEL_HI;
    w.inactive.bg_stroke = Stroke::new(1.0, BORDER);
    w.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    w.inactive.corner_radius = r;

    w.hovered.bg_fill = Color32::from_rgb(0x15, 0x24, 0x1F);
    w.hovered.weak_bg_fill = Color32::from_rgb(0x15, 0x24, 0x1F);
    w.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x2C, 0x5A, 0x46));
    w.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = GREEN_DIM;
    w.active.weak_bg_fill = GREEN_DIM;
    w.active.bg_stroke = Stroke::new(1.0, GREEN);
    w.active.fg_stroke = Stroke::new(1.0, GREEN);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open.bg_fill = PANEL_HI;
    w.open.weak_bg_fill = PANEL_HI;
    w.open.bg_stroke = Stroke::new(1.0, GREEN);
    w.open.fg_stroke = Stroke::new(1.0, TEXT);
    w.open.corner_radius = r;
    v
}

/// Primary call-to-action button (green outline, subtle glow fill).
pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        egui::RichText::new(text.into())
            .color(GREEN)
            .font(ui_bold(14.0)),
    )
    .fill(Color32::from_rgb(0x0F, 0x2A, 0x1D))
    .stroke(Stroke::new(1.0, GREEN))
    .min_size(egui::vec2(0.0, 32.0))
}

/// Secondary button.
pub fn secondary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.into()).color(TEXT))
        .fill(PANEL_HI)
        .stroke(Stroke::new(1.0, BORDER))
        .min_size(egui::vec2(0.0, 32.0))
}

/// Destructive button (red outline).
pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.into()).color(RED))
        .fill(Color32::from_rgb(0x2A, 0x10, 0x13))
        .stroke(Stroke::new(1.0, RED))
        .min_size(egui::vec2(0.0, 32.0))
}

/// A small status dot with a restrained glow.
pub fn glow_dot(painter: &egui::Painter, center: egui::Pos2, color: Color32) {
    painter.circle_filled(center, 7.0, color.gamma_multiply(0.10));
    painter.circle_filled(center, 4.5, color.gamma_multiply(0.25));
    painter.circle_filled(center, 3.0, color);
}

/// A full-width, left-aligned selectable row (sidebar items).
pub fn row_button(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    selected: bool,
    indent: f32,
) -> egui::Response {
    let text: egui::WidgetText = text.into();
    let label = text.text().to_owned();
    let h = 26.0;
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, true, selected, label.clone())
    });
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            GREEN_DIM
        } else if resp.hovered() {
            Color32::from_rgb(0x13, 0x20, 0x1C)
        } else {
            Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, CornerRadius::same(5), fill);
        if selected {
            let bar = egui::Rect::from_min_size(
                rect.min + egui::vec2(0.0, 5.0),
                egui::vec2(2.0, h - 10.0),
            );
            ui.painter().rect_filled(bar, 1.0, GREEN);
        }
        if resp.has_focus() {
            ui.painter().rect_stroke(
                rect,
                CornerRadius::same(5),
                Stroke::new(1.0, CYAN),
                egui::StrokeKind::Inside,
            );
        }
        let color = if selected { GREEN } else { TEXT };
        let galley = text.into_galley(
            ui,
            Some(egui::TextWrapMode::Truncate),
            w - indent - 12.0,
            egui::TextStyle::Body,
        );
        let pos = egui::pos2(
            rect.left() + 8.0 + indent,
            rect.center().y - galley.size().y / 2.0,
        );
        ui.painter().galley(pos, galley, color);
    }
    resp
}

/// Section label in the sidebar ("FOLDERS", "TAGS", …).
pub fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(text)
            .font(mono(10.5))
            .color(MUTED)
            .extra_letter_spacing(1.2),
    );
}
