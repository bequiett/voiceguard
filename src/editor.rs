use std::sync::Arc;

use nih_plug::prelude::*;
use nih_plug_egui::{
    create_egui_editor,
    egui::{self, Color32, FontId, RichText, Stroke, Vec2},
    widgets, EguiState,
};

use crate::VoiceGuardParams;

pub fn create(params: Arc<VoiceGuardParams>) -> Option<Box<dyn Editor>> {
    let state = params.editor_state.clone();
    create_egui_editor(
        state,
        (),
        |ctx, _| {
            let mut visuals = egui::Visuals::dark();
            visuals.window_rounding = 0.0.into();
            visuals.menu_rounding = 0.0.into();
            visuals.widgets.noninteractive.rounding = 0.0.into();
            visuals.widgets.inactive.rounding = 0.0.into();
            visuals.widgets.hovered.rounding = 0.0.into();
            visuals.widgets.active.rounding = 0.0.into();
            visuals.widgets.open.rounding = 0.0.into();
            visuals.panel_fill = Color32::from_rgb(18, 20, 23);
            visuals.extreme_bg_color = Color32::from_rgb(11, 13, 15);
            visuals.widgets.inactive.bg_fill = Color32::from_rgb(31, 34, 39);
            visuals.widgets.hovered.bg_fill = Color32::from_rgb(42, 46, 52);
            visuals.widgets.active.bg_fill = Color32::from_rgb(54, 60, 68);
            visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, Color32::from_rgb(58, 63, 70));
            ctx.set_visuals(visuals);
        },
        move |ctx, setter, _| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 9.0);
                ui.spacing_mut().slider_width = 280.0;
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.label(RichText::new("VOICEGUARD").font(FontId::proportional(22.0)).strong());
                        ui.label(RichText::new("REAL-TIME VOICE CLEANUP  /  48 kHz").size(10.0).color(Color32::from_gray(145)));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(14.0);
                        ui.add(widgets::ParamSlider::for_param(&params.output_gain, setter));
                        ui.label(RichText::new("OUTPUT").size(10.0).color(Color32::from_gray(160)));
                    });
                });
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(8.0);

                egui::Grid::new("voiceguard-controls")
                    .num_columns(2)
                    .spacing(Vec2::new(24.0, 12.0))
                    .min_col_width(250.0)
                    .show(ui, |ui| {
                        control(ui, "CLEANUP", "Neural suppression amount", &params.strength, setter);
                        control(ui, "VOICE PROTECT", "Back off around speech-like detail", &params.voice_protect, setter);
                        ui.end_row();
                        control(ui, "EVENT", "Transient / breath / wind rejection", &params.artifact, setter);
                        control(ui, "FLOOR", "Maximum event attenuation floor", &params.floor, setter);
                        ui.end_row();
                    });

                ui.add_space(12.0);
                ui.separator();
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(4.0);
                    ui.add(widgets::ParamSlider::for_param(&params.bypass, setter));
                    ui.label(RichText::new("VOICE-FIRST / 60 ms pipeline").size(10.0).color(Color32::from_gray(125)));
                });
            });
        },
    )
}

fn control<P: Param>(ui: &mut egui::Ui, title: &str, help: &str, param: &P, setter: &ParamSetter) {
    ui.vertical(|ui| {
        ui.label(RichText::new(title).size(11.0).strong());
        ui.label(RichText::new(help).size(9.0).color(Color32::from_gray(125)));
        ui.add(widgets::ParamSlider::for_param(param, setter));
    });
}
