use chrono::{DateTime, Local, TimeZone};
use eframe::egui::{self, Color32};
use egui_plot::{Bar, BarChart, GridMark, Plot};
use crate::state::{AppState, telemetry::TelemetryType};

pub fn show(ctx: &egui::Context, state: &mut AppState, tt: TelemetryType) {
  let (title, chart_id) = match tt {
    TelemetryType::BattarCharge => ("Заряд АКБ", "battary_charage"),
    TelemetryType::SolarVa => ("Мощность СП (V*A)", "solar_va"),
    TelemetryType::ChargingVa => ("Мощность заряда АКБ (V*A)", "charging_va"),
    TelemetryType::DeviceConsumption => ("Общее потребление устройства (V*A)", "device_consumtion"),
    TelemetryType::Uptime => ("Время работы", "uptime"),
    };

  egui::Window::new(title)
    .default_pos([320.0, 20.0])
    .default_size([500.0, 350.0])
    .resizable(true)
    .show(ctx, |ui| {
      let telemetry_opt = state.telemetry.get(&tt);
      let telemetry = telemetry_opt.as_ref().unwrap();
      let t0_ns = telemetry.started_at.unwrap_or_else(|| {
          telemetry.points.first().map(|p| p.timestamp).unwrap_or(0)
      });

      let t0_dt: DateTime<Local> = Local
        .timestamp_opt(
          (t0_ns / 1_000_000_000) as i64,
          (t0_ns % 1_000_000_000) as u32,
        )
        .single()
        .unwrap_or_else(|| Local.timestamp_opt(0, 0).unwrap());

      let step_secs = 5.0;

      // let y_min = state.telemetry.points.iter().map(|p| p.value)
      //     .fold(f64::INFINITY, f64::min);
      // let y_max = state.telemetry.points.iter().map(|p| p.value)
      //     .fold(f64::NEG_INFINITY, f64::max);
      // let pad  = (y_max - y_min).max(0.5) * 0.1;
      // let y_lo = y_min - pad;
      // let y_hi = y_max + pad;

      let bars: Vec<Bar> = telemetry.points
        .iter()
        .map(|p| {
            let x = (p.timestamp - t0_ns) as f64 * 1e-9;
            Bar::new(x, p.value)
              .fill(bar_color(p.value, tt))
              .width(step_secs * 1.0) 
        })
        .collect();

      let chart = BarChart::new(chart_id, bars)
        .element_formatter(Box::new(move |bar, _chart| {
            let dt = t0_dt
              + chrono::TimeDelta::nanoseconds((bar.argument * 1_000_000_000.0) as i64);
            format_value(bar.value, dt, tt)
          }
      ));

      Plot::new("telemetry_plot")
        .allow_zoom(true)
        .allow_drag(true)
        .x_axis_formatter(move |mark: GridMark, _range| {
            let dt = t0_dt + chrono::TimeDelta::nanoseconds((mark.value * 1_000_000_000.0) as i64);
            dt.format("%d.%m.%y %H:%M:%S").to_string()
        })
        .show(ui, |plot_ui| {
            plot_ui.bar_chart(chart);

            // let b = plot_ui.plot_bounds();
            // if b.min()[1] < y_lo - 0.5 * pad {
            //     plot_ui.set_plot_bounds(PlotBounds::from_min_max(
            //         [b.min()[0], y_lo],
            //         [b.max()[0], y_hi],
            //     ));
            // }
          });
        });
}

fn bar_color(value: f64, tt: TelemetryType) -> Color32 {
  match tt {
    TelemetryType::BattarCharge => {
      let p = value.clamp(0.0, 100.0);
      let (r, g) = if p <= 50.0 {
        // 0..50: R=255, G растёт 0→255
        let t = p / 50.0;
        (255.0, 255.0 * t)
      } else {
        // 50..100: G=255, R падает 255→0
        let t = (p - 50.0) / 50.0;
        (255.0 * (1.0 - t), 255.0)
      };
      Color32::from_rgb(r as u8, g as u8, 0)
    },
    TelemetryType::SolarVa => {
      let p = value.clamp(0.0, 5.0);
      let (r, g) = if p <= 50.0 {
        // 0..50: R=255, G растёт 0→255
        let t = p / 50.0;
        (255.0, 255.0 * t)
      } else {
        // 50..100: G=255, R падает 255→0
        let t = (p - 50.0) / 50.0;
        (255.0 * (1.0 - t), 255.0)
      };
      Color32::from_rgb(r as u8, g as u8, 0)
    },
    TelemetryType::ChargingVa => Color32::LIGHT_GREEN,
    TelemetryType::DeviceConsumption => Color32::LIGHT_BLUE,
    TelemetryType::Uptime => Color32::BLUE,
  }
}

fn format_value(value: f64, dt: DateTime<Local>, tt: TelemetryType) -> String {
  match tt {
    TelemetryType::BattarCharge => {
      format!(
        "{:.2}% — {}",
        value,
        dt.format("%d.%m.%y %H:%M:%S")
      )
    },
    TelemetryType::SolarVa => {
      format!(
        "{:.4}V*A — {}",
        value,
        dt.format("%d.%m.%y %H:%M:%S")
      )
    }
    TelemetryType::ChargingVa => {
      format!(
        "{:.4}V*A — {}",
        value,
        dt.format("%d.%m.%y %H:%M:%S")
      )
    }
    TelemetryType::DeviceConsumption => {
      format!(
        "{:.4}V*A — {}",
        value,
        dt.format("%d.%m.%y %H:%M:%S")
      )
    }
    TelemetryType::Uptime => "ON".into(),
  }
  
}