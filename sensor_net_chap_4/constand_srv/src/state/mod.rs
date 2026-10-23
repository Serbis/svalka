pub mod telemetry;

use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use telemetry::DeviceTelemetry;

use crate::state::telemetry::TelemetryType;

pub struct AppState {
  pub telemetry: HashMap<TelemetryType, DeviceTelemetry>,
  pub paused: Arc<AtomicBool>
}

impl AppState {
  pub fn new(paused: Arc<AtomicBool>) -> Self {
    let mut telemetry = HashMap::new();
    telemetry.insert(TelemetryType::BattarCharge, DeviceTelemetry::default());
    telemetry.insert(TelemetryType::ChargingVa, DeviceTelemetry::default());
    telemetry.insert(TelemetryType::DeviceConsumption, DeviceTelemetry::default());
    telemetry.insert(TelemetryType::SolarVa, DeviceTelemetry::default());
    telemetry.insert(TelemetryType::Uptime, DeviceTelemetry::default());

    Self {
      telemetry,
      paused,
    }
  }
}