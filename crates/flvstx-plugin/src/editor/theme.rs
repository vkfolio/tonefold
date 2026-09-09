//! Shared visual language for the editor.
use nih_plug_egui::egui::{self, Color32, RichText};
pub const CANVAS: Color32 = Color32::from_rgb(16, 21, 28);
pub const PANEL: Color32 = Color32::from_rgb(22, 28, 37);
pub const SURFACE: Color32 = Color32::from_rgb(30, 38, 49);
pub const BORDER: Color32 = Color32::from_rgb(47, 59, 73);
pub const TEXT: Color32 = Color32::from_rgb(229, 235, 243);
pub const MUTED: Color32 = Color32::from_rgb(146, 164, 183);
pub const ACCENT: Color32 = Color32::from_rgb(116, 221, 196);
pub fn panel_frame() -> egui::Frame {
    egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(14, 12))
}
pub fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).small().strong().color(MUTED));
}
pub fn brand(ui: &mut egui::Ui, scale: f32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0) * scale, egui::Sense::hover());
    ui.painter().rect_filled(r, 7.0, ACCENT);
    for (i, h) in [9.0, 17.0, 12.0].iter().enumerate() {
        let x = r.left() + (7.0 + i as f32 * 6.0) * scale;
        ui.painter().rect_filled(egui::Rect::from_center_size(egui::pos2(x, r.center().y), egui::vec2(3.0, *h) * scale), 1.0, CANVAS);
    }
    ui.label(RichText::new("FLVSTX").size(19.0 * scale).strong().color(TEXT));
}
