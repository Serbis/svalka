use crate::state::telemetry::{TelemetryPoint, TelemetryType};

#[derive(Debug)]
pub enum Message {
  TelemetryPoint(TelemetryPoint, TelemetryType),
  TelemetryPointPack(Vec<TelemetryPoint>, TelemetryType),
}