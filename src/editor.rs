use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Color32, FontId, RichText, Stroke, Vec2},
    widgets,
};

use crate::VoiceGuardParams;

pub fn create(params: Arc<VoiceGuardParams>) -> Option<Box<dyn Editor>> {
    let state = params.editor_state.clone();
    create_egui_editor(
        state,
        (),
        |ctx, _| {
            let mut visuals = egui::Visuals::light();
            visuals.panel_fill = Color32::from_rgb(246, 247, 249);
            visuals.extreme_bg_color = Color32::from_rgb(232, 235, 239);
            visuals.window_fill = Color32::from_rgb(246, 247, 249);
            visuals.faint_bg_color = Color32::from_rgb(239, 241, 244);
            visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(246, 247, 249);
            visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(42, 47, 54));
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(205, 210, 216));
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(229, 233, 238);
            visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(38, 43, 49));
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(216, 222, 229);
            visuals.widgets.active.bg_fill = Color32::from_rgb(202, 210, 219);
            visuals.selection.bg_fill = Color32::from_rgb(52, 114, 168);
            visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
            ctx.set_visuals(visuals);
        },
        move |ctx, setter, _| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(12.0, 10.0);
                ui.spacing_mut().slider_width = 250.0;

                ui.add_space(18.0);
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("VOICEGUARD")
                                .font(FontId::proportional(28.0))
                                .color(Color32::from_rgb(24, 28, 33)),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new("REAL-TIME VOICE CLEANUP  ·  48 kHz  ·  60 ms")
                                .size(10.0)
                                .color(Color32::from_rgb(104, 111, 120)),
                        );
                    });
                });

                ui.add_space(18.0);
                ui.separator();
                ui.add_space(18.0);

                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.set_width(290.0);
                        section(ui, "CLEANUP", "Neural suppression amount", &params.strength, setter);
                        ui.add_space(24.0);
                        section(ui, "EVENT", "Transient / breath / wind rejection", &params.artifact, setter);
                    });

                    ui.add_space(34.0);

                    ui.vertical(|ui| {
                        ui.set_width(290.0);
                        section(ui, "VOICE PROTECT", "Preserve speech-like detail", &params.voice_protect, setter);
                        ui.add_space(24.0);
                        section(ui, "FLOOR", "Maximum event attenuation floor", &params.floor, setter);
                    });
                });

                ui.add_space(22.0);
                ui.separator();
                ui.add_space(14.0);

                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("OUTPUT")
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(47, 53, 60)),
                        );
                        ui.label(
                            RichText::new("Post-processing gain")
                                .size(9.0)
                                .color(Color32::from_rgb(112, 119, 128)),
                        );
                        ui.add(widgets::ParamSlider::for_param(&params.output_gain, setter));
                    });

                    ui.add_space(34.0);

                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("BYPASS")
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(47, 53, 60)),
                        );
                        ui.label(
                            RichText::new("Disable processing")
                                .size(9.0)
                                .color(Color32::from_rgb(112, 119, 128)),
                        );
                        ui.add(widgets::ParamSlider::for_param(&params.bypass, setter));
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
    ui.label(
        RichText::new(title)
            .size(11.0)
            .strong()
            .color(Color32::from_rgb(47, 53, 60)),
    );
    ui.label(
        RichText::new(help)
            .size(9.0)
            .color(Color32::from_rgb(112, 119, 128)),
    );
    ui.add_space(4.0);
    ui.add(widgets::ParamSlider::for_param(param, setter));
}
