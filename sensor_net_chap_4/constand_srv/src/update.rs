use crate::commands::Message;
use crate::state::AppState;

pub fn handle(state: &mut AppState, msg: Message) {
  match msg {
    Message::TelemetryPoint(p, tt) => {
      let mut telemetry_opt = state.telemetry.get_mut(&tt);
      let telemetry = telemetry_opt.as_mut().unwrap();

      if telemetry.started_at.is_none() {
        telemetry.started_at = Some(p.timestamp);
      }
      telemetry.latest = Some(p.value);
      telemetry.points.push(p);
    }
    Message::TelemetryPointPack(p, tt) => {
      let mut telemetry_opt = state.telemetry.get_mut(&tt);
      let telemetry = telemetry_opt.as_mut().unwrap();

      if telemetry.started_at.is_none() {
        telemetry.started_at = Some(p.first().unwrap().timestamp);
      }
      telemetry.latest = Some(p.last().unwrap().value);
      telemetry.points.extend(p);
    }
  }
}