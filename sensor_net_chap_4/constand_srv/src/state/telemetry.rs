#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TelemetryType {
  BattarCharge,
  SolarVa,
  ChargingVa,
  DeviceConsumption,
  Uptime
}

#[derive(Clone, Debug)]
pub struct TelemetryPoint {
  pub timestamp: u64,
  pub value: f64,
}

#[derive(Default)]
pub struct DeviceTelemetry {
  pub started_at: Option<u64>,   
  pub points: Vec<TelemetryPoint>,
  pub latest: Option<f64>
}