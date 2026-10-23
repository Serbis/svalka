use std::sync::atomic::{AtomicBool};
use std::sync::Arc;
use prost::Message as _;
use tokio::sync::mpsc;
use crate::commands::Message;
use crate::proto;
use crate::state::telemetry::{TelemetryPoint, TelemetryType};
use rumqttc::{AsyncClient, Event, MqttOptions, Packet, QoS};
use tsink::{DataPoint, Label, Row, Storage, StorageBuilder, TimestampPrecision};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{info, warn};

pub fn spawn(tx: mpsc::Sender<Message>, _: Arc<AtomicBool>) {
  crate::tokio_handle().spawn(async move {
    let storage = StorageBuilder::new()
    .with_data_path("./tsink-data")
    .with_timestamp_precision(TimestampPrecision::Nanoseconds)
    .build()
    .unwrap();

    // --- Extract Telemtery from DB ----
    let battary_charge_telemetry = load_telemtery_from_db(storage.clone(), "battary_charge");
    let solar_va_telemetry = load_telemtery_from_db(storage.clone(), "solar_va");
    let uptime_telemetry = load_telemtery_from_db(storage.clone(), "uptime");
    let charging_va_telemetry = load_telemtery_from_db(storage.clone(), "charging_va");
    let device_consumtion_telemetry = load_telemtery_from_db(storage.clone(), "device_consumtion");
    
    let _ = tx.send(Message::TelemetryPointPack(battary_charge_telemetry, TelemetryType::BattarCharge)).await;
    let _ = tx.send(Message::TelemetryPointPack(solar_va_telemetry, TelemetryType::SolarVa)).await;
    let _ = tx.send(Message::TelemetryPointPack(uptime_telemetry, TelemetryType::Uptime)).await;
    let _ = tx.send(Message::TelemetryPointPack(charging_va_telemetry, TelemetryType::ChargingVa)).await;
    let _ = tx.send(Message::TelemetryPointPack(device_consumtion_telemetry, TelemetryType::DeviceConsumption)).await;
    
    
    // --- Mqtt connection ---
    let mut mqttoptions = MqttOptions::new(
      "rust-telemetry-consumer",
      "192.168.1.7",
      1883,
    );
    mqttoptions.set_credentials("telemetry_user", "StrongPass123!");
    mqttoptions.set_keep_alive(Duration::from_secs(10));
    
    let (client, mut eventloop) = AsyncClient::new(mqttoptions, 10);
    
    client.subscribe("#", QoS::AtLeastOnce).await.unwrap();
    info!("Subscribed to #");
    
    // --- Telemetry loop, recevice, store, draw, log ---
    loop {
      match eventloop.poll().await {
        Ok(Event::Incoming(Packet::Publish(publish))) => {
          let topic = publish.topic.clone();          
          let telemetry = proto::telemetry::Telemetry::decode(&publish.payload[..]).unwrap();
          
          let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64;
          
          let row_uptime = Row::with_labels(
            "uptime",
            vec![
            Label::new("node_id", &topic),
            ],
            DataPoint::new(now, telemetry.uptime)
          );
          let row_solar_va = Row::with_labels(
            "solar_va",
            vec![
            Label::new("node_id", &topic),
            ],
            DataPoint::new(now, telemetry.solar_va as f64)
          );
          let row_charging_va = Row::with_labels(
              "charging_va",
              vec![
              Label::new("node_id", &topic),
            ],
            DataPoint::new(now, telemetry.charging_va as f64)
          );
          let row_battary_charge = Row::with_labels(
            "battary_charge",
            vec![
            Label::new("node_id", &topic),
            ],
            DataPoint::new(now, telemetry.battary_charge as f64)
          );
          let row_device_consumtion = Row::with_labels(
            "device_consumtion",
            vec![
            Label::new("node_id", &topic),
            ],
            DataPoint::new(now, telemetry.device_consumtion as f64)
          );
          
          storage.insert_rows(&[
            row_uptime,
            row_solar_va,
            row_charging_va,
            row_battary_charge,
            row_device_consumtion
          ]).unwrap();
          
          let battary_charge_point = TelemetryPoint {
            timestamp: now as u64,
            value: telemetry.battary_charge as f64 
          };
          let uptime_point = TelemetryPoint {
            timestamp: now as u64,
            value: telemetry.uptime as f64 
          };
          let solar_va_point = TelemetryPoint {
            timestamp: now as u64,
            value: telemetry.solar_va as f64 
          };
          let charging_va_point = TelemetryPoint {
            timestamp: now as u64,
            value: telemetry.charging_va as f64 
          };
          let device_consumtion_point = TelemetryPoint {
            timestamp: now as u64,
            value: telemetry.device_consumtion as f64 
          };

          let _ = tx.send(Message::TelemetryPoint(battary_charge_point, TelemetryType::BattarCharge)).await;
          let _ = tx.send(Message::TelemetryPoint(uptime_point, TelemetryType::Uptime)).await;
          let _ = tx.send(Message::TelemetryPoint(solar_va_point, TelemetryType::SolarVa)).await;
          let _ = tx.send(Message::TelemetryPoint(charging_va_point, TelemetryType::ChargingVa)).await;
          let _ = tx.send(Message::TelemetryPoint(device_consumtion_point, TelemetryType::DeviceConsumption)).await;

          info!(
            topic = %topic,
            telemetry = ?telemetry,
            "Telemetry received"
          );
          }
          Ok(Event::Incoming(other)) => {
            if matches!(other, Packet::ConnAck(_)) {
              info!("Connected to MQTT broker");
            }
          }
          Ok(_) => {} 
          Err(e) => {
            warn!(error = %e, "MQTT event loop error, will retry...");
            tokio::time::sleep(Duration::from_secs(1)).await;
          }
        }
      }
    });
  }
  
  fn load_telemtery_from_db(storage: Arc<dyn Storage>, metric: &str) -> Vec<TelemetryPoint> {
    let rows_battary_charge = storage
      .select_all(metric, 0, i64::MAX)
      .unwrap();
    rows_battary_charge.first()
      .unwrap()
      .1
      .iter()
      .map(|p| {
        TelemetryPoint {
          timestamp: p.timestamp as u64,
          value: p.value.as_f64().unwrap() 
        }
      })
      .collect()
  }