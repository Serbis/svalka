pub mod windows;

use eframe::egui;
use crate::state::{AppState, telemetry::TelemetryType};

pub fn show(ctx: &egui::Context, state: &mut AppState) {
  windows::graph::show(ctx, state, TelemetryType::BattarCharge);
  windows::graph::show(ctx, state, TelemetryType::SolarVa);
  windows::graph::show(ctx, state, TelemetryType::DeviceConsumption);
  windows::graph::show(ctx, state, TelemetryType::ChargingVa);
}
