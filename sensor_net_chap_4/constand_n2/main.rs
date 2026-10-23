#![no_std]
#![no_main]
#![deny(
  clippy::mem_forget,
  reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use defmt::{error, info};
use alloc::boxed::Box;
use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp_hal::aes::AesBackend;
use esp_hal::peripherals::{AES};
use esp_hal::{clock::CpuClock, efuse::MacAddress};
use esp_hal::timer::timg::TimerGroup;
use embassy_net::{IpAddress, Runner, StackResources};
use esp_radio::wifi::{AuthenticationMethodConfig, Config, Interface, Ssid, WifiController};
use esp_radio::wifi::sta::StationConfig;
use panic_rtt_target as _;
use esp_radio::wifi::ControllerConfig;
use esp_hal::rng::Rng;
use alloc::sync::Arc;
use crate::mqtt_gateway::MqttGateway;
use crate::ntp_sync_source::NtpSyncSource;
use crate::sync_source::SyncSource;
use burelom_net::burelom_node::{BurelomNode};
use burelom_net::roles::device_roles::DeviceRole;
use crate::esp_now_mac::EspNowMac;
use crate::esp32_cipher::Esp32Cypher;
use crate::esp32_rand::Esp32Rand;
use esp_hal::gpio::Pin;

#[cfg(feature = "node_1")]
static NODE_ADDR: u32 = 0x01;
#[cfg(feature = "node_2")]
static NODE_ADDR: u32 = 0x02;
#[cfg(feature = "node_3")]
static NODE_ADDR: u32 = 0x03;

const SSID: &str = "MTS_GPON_1313";
const PASSWORD: &str = "***";
const NTP_SERVER: &str = "pool.ntp.org";
const NTP_PORT: u16 = 123;

extern crate alloc;

#[path = "../esp32_cipher.rs"]
mod esp32_cipher;
#[path = "../esp_now_mac.rs"]
mod esp_now_mac;
#[path = "../esp32_rand.rs"]
mod esp32_rand;
#[path = "../ntp_sync_source.rs"]
mod ntp_sync_source;
#[path = "../sync_source.rs"]
mod sync_source;
#[path = "../mqtt_producer.rs"]
mod mqtt_producer;
#[path = "../mqtt_gateway.rs"]
mod mqtt_gateway;

// This creates a default app-descriptor required by the esp-idf bootloader.
// For more information see: <https://docs.espressif.com/projects/esp-idf/en/stable/esp32/api-reference/system/app_image_format.html#application-description>
esp_bootloader_esp_idf::esp_app_desc!();

#[allow(
  clippy::large_stack_frames,
  reason = "it's not unusual to allocate larger buffers etc. in main"
)]
#[esp_rtos::main]
async fn main(spawner: Spawner) -> ! {
  rtt_target::rtt_init_defmt!();
  
  let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
  let peripherals = esp_hal::init(config);
  
  esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 65536);
  esp_alloc::heap_allocator!(size: 32768); 

  let timg0 = TimerGroup::new(peripherals.TIMG0);
  esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
  
  info!("Embassy initialized!");
  
  // Wifi initializtion
  // Setup new mac
  esp_hal::efuse::override_mac_address(
    MacAddress::new_eui48([0x02_u8, 0x00, 0x00, 0x00, 0x00, NODE_ADDR as u8])
  ).unwrap();
  
  // -------- WiFi --------
  // Controller and IF
  let sta_interface = esp_radio::wifi::Interface::station();
  let wifi_controller = esp_radio::wifi::WifiController::new(
    peripherals.WIFI,
    ControllerConfig::default(),
  )
  .unwrap();
  let wifi_controller: &'static mut WifiController<'static> =
    Box::leak(Box::new(wifi_controller));
  wifi_controller.set_channel(3, esp_radio::wifi::SecondaryChannel::Above).unwrap();
  let esp_now = wifi_controller.esp_now();
  esp_now.set_channel(3).unwrap();
  
  // Network stack
  let net_config = embassy_net::Config::dhcpv4(Default::default());
  let rng = Rng::new();
  let seed = (rng.random() as u64) << 32 | rng.random() as u64;
  
  let stack_resources: &'static mut StackResources<5> =
  Box::leak(Box::new(StackResources::<5>::new()));
  
  let (stack, runner) = embassy_net::new(
    sta_interface,
    net_config,
    stack_resources,
    seed,
  );
  
  // Run tasks
  spawner.spawn(aes_backend_task(peripherals.AES).unwrap());
  spawner.spawn(connection_task(wifi_controller).unwrap());
  spawner.spawn(net_task(runner).unwrap());
  
  
  // Sync source
  let ntp_sync_source = Arc::new(
    NtpSyncSource::builder()
    .ntp_server(NTP_SERVER.into())
    .ntp_port(NTP_PORT)
    .recall_interval(Duration::from_secs(10))
    .stack(stack)
    .spawner(&spawner)
    .build()
  );
  let _ = ntp_sync_source.get_instance();
  
  // Burelom node
  let key: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
  let cipher = Esp32Cypher::new(key);
  let led_pin = peripherals.GPIO3.degrade();
  let mac = EspNowMac::new(esp_now, led_pin, &spawner);
  let rand = Esp32Rand::new();
  let gateway = MqttGateway::builder()
    .broker_addr(IpAddress::v4(192, 168, 1, 7))
    .broker_port(1883)
    .broker_user("telemetry_user")
    .broker_password("StrongPass123!")
    .node_addr(NODE_ADDR)
    .stack(stack)
    .spawner(&spawner)
    .build();
  
  let node = BurelomNode::builder()
    .addr(NODE_ADDR)
    .roles(DeviceRole::DEFAULT | DeviceRole::BEACON | DeviceRole::GATEWAY)
    .mac(Box::new(mac))
    .cipher(Box::new(cipher))
    .gateway(Arc::new(gateway))
    .hello_interval(Duration::from_secs(3))
    .route_ttl(Duration::from_secs(25))
    .rand(Box::new(rand))
    .beacon_interval(Duration::from_secs(30))
    .build()
    .unwrap();
  node.run(&spawner);

  loop {
    Timer::after_secs(1).await;
  }
}

#[embassy_executor::task]
async fn aes_backend_task(aes_peripheral: AES<'static>) {
  let mut backend = AesBackend::new(aes_peripheral);
  let _driver = backend.start();
  core::future::pending::<()>().await;
}

#[embassy_executor::task]
async fn connection_task(controller: &'static mut WifiController<'static>) {
  info!("Wi-Fi connection task started");
  
  let station_config = StationConfig::default()
  .with_ssid(Ssid::try_from(SSID).unwrap())
  .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
    PASSWORD.try_into().unwrap(),
  ));
  
  let client_config = Config::Station(station_config);
  
  controller.set_config(&client_config).unwrap();
  
  loop {
    match controller.connect_async().await {
      Ok(_) => {
        info!("Wi-Fi connected successfully");
        let _ = controller
        .wait_for_disconnect_async()
        .await;
        info!("Wi-Fi disconnected");
      }
      Err(e) => {
        error!("Failed to connect to Wi-Fi: {:?}", e);
        Timer::after(Duration::from_secs(5)).await;
      }
    }
  }
}

#[embassy_executor::task]
async fn net_task(mut runner: Runner<'static, Interface>) {
  runner.run().await
}
