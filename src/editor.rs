use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Color32, FontId, RichText, Stroke, Vec2},
    widgets,
};

use crate::VoiceGuardParams;

const TEXT: Color32 = Color32::from_rgb(28, 32, 37);
const MUTED: Color32 = Color32::from_rgb(104, 111, 120);
const BORDER: Color32 = Color32::from_rgb(205, 210, 216);

pub fn create(params: Arc<VoiceGuardParams>) -> Option<Box<dyn Editor>> {
    let state = params.editor_state.clone();
    create_egui_editor(
        state,
        (),
        |ctx, _| {
            let mut visuals = egui::Visuals::light();
            visuals.override_text_color = Some(TEXT);
            visuals.panel_fill = Color32::from_rgb(248, 249, 250);
            visuals.window_fill = Color32::from_rgb(248, 249, 250);
            visuals.extreme_bg_color = Color32::from_rgb(232, 235, 239);
            visuals.faint_bg_color = Color32::from_rgb(242, 244, 246);
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(248, 249, 250);
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(231, 235, 239);
            visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(188, 194, 201));
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(218, 224, 230);
            visuals.widgets.active.bg_fill = Color32::from_rgb(204, 212, 220);
            visuals.selection.bg_fill = Color32::from_rgb(35, 103, 153);
            visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
            ctx.set_visuals(visuals);
        },
        move |ctx, setter, _| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(8.0, 6.0);
                ui.spacing_mut().slider_width = 228.0;

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("VOICEGUARD")
                                .font(FontId::proportional(24.0))
                                .color(TEXT),
                        );
                        ui.label(
                            RichText::new("REAL-TIME VOICE CLEANUP")
                                .size(9.5)
                                .color(MUTED),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(16.0);
                        ui.label(
                            RichText::new("48 kHz  /  60 ms")
                                .size(9.5)
                                .color(MUTED),
                        );
                    });
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.set_width(250.0);
                        section(ui, "CLEANUP", "Neural suppression", &params.strength, setter);
                        ui.add_space(14.0);
                        section(ui, "EVENT", "Transient / breath / wind rejection", &params.artifact, setter);
                    });

                    ui.add_space(18.0);

                    ui.vertical(|ui| {
                        ui.set_width(250.0);
                        section(ui, "VOICE PROTECT", "Preserve speech detail", &params.voice_protect, setter);
                        ui.add_space(14.0);
                        section(ui, "FLOOR", "Maximum event attenuation floor", &params.floor, setter);
                    });
                });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(9.0);

                ui.horizontal(|ui| {
                    ui.add_space(16.0);
                    ui.vertical(|ui| {
                        ui.set_width(250.0);
                        section(ui, "OUTPUT", "Post-processing gain", &params.output_gain, setter);
                    });

                    ui.add_space(18.0);

                    ui.vertical(|ui| {
                        ui.set_width(250.0);
                        section(ui, "BYPASS", "Disable VoiceGuard processing", &params.bypass, setter);
                    });
                });
            });
        },
    )
}

fn section<P: Param>(
    ui: &mut egui::Ui,
    title: &str,
    help: &str,
    param: &P,
    setter: &ParamSetter,
) {
    ui.label(RichText::new(title).size(10.5).strong().color(TEXT));
    ui.label(RichText::new(help).size(9.0).color(MUTED));
    ui.add_space(2.0);
    ui.add(widgets::ParamSlider::for_param(param, setter));
}
