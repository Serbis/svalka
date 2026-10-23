#![no_std]
#![no_main]
#![deny(
  clippy::mem_forget,
  reason = "mem::forget is generally not safe to do with esp_hal types, especially those \
    holding buffers for the duration of a data transfer."
)]
#![deny(clippy::large_stack_frames)]

use burelom_net::burelom_node::{BurelomNode};
use burelom_net::roles::device_roles::DeviceRole;
use defmt::{info};
use alloc::boxed::Box;
use embassy_executor::Spawner;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_sync::mutex::Mutex;
use embassy_time::{Duration, Timer};
use esp_hal::aes::AesBackend;
use esp_hal::peripherals::{AES};
use esp_hal::{clock::CpuClock, efuse::MacAddress};
use esp_hal::timer::timg::TimerGroup;
use alloc::vec::Vec;
use esp_hal::{
    i2c::master::{Config as I2cConfig, I2c},
    time::Rate,
};
use static_cell::StaticCell;
use esp_hal::gpio::Pin;
use esp_radio::wifi::WifiController;
use panic_rtt_target as _;
use esp_radio::wifi::ControllerConfig;
use crate::esp_now_mac::EspNowMac;
use crate::esp32_cipher::Esp32Cypher;
use crate::esp32_rand::Esp32Rand;
use crate::nmea_sync_source::NmeaSyncSource;
use crate::power_manager::PowerManager;
use esp_hal::uart::Uart;
use esp_hal::Async;
use prost::Message as _;

type I2cBus = Mutex<NoopRawMutex, I2c<'static, esp_hal::Async>>;


 #[cfg(feature = "node_1")]
static NODE_ADDR: u32 = 0x01;
 #[cfg(feature = "node_2")]
static NODE_ADDR: u32 = 0x02;
 #[cfg(feature = "node_3")]
static NODE_ADDR: u32 = 0x03;
static I2C_BUS: StaticCell<I2cBus> = StaticCell::new();

extern crate alloc;

#[path = "../esp32_cipher.rs"]
mod esp32_cipher;
#[path = "../esp_now_mac.rs"]
mod esp_now_mac;
#[path = "../esp32_rand.rs"]
mod esp32_rand;
#[path = "../ntp_sync_source.rs"]
mod ntp_sync_source;
#[path = "../nmea_sync_source.rs"]
mod nmea_sync_source;
#[path = "../eps_now_sync_source.rs"]
mod esp_now_sync_source;
#[path = "../sync_source.rs"]
mod sync_source;
#[path = "../slot_blinker.rs"]
mod slot_blinker;
#[path = "../power_manager.rs"]
mod power_manager;
#[path = "../proto/telemetry.rs"]
mod proto;


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
  
  let timg0 = TimerGroup::new(peripherals.TIMG0);
  esp_rtos::start(timg0.timer0, peripherals.FROM_CPU_INTR0);
  
  info!("Embassy initialized!");
  
  // UART 
  let uart1_config = esp_hal::uart::Config::default()
  .with_rx(esp_hal::uart::RxConfig::default().with_fifo_full_threshold(64 as u16))
  .with_baudrate(9600);

  let uart1: Uart<'static, Async> = Uart::new(peripherals.UART1, uart1_config)
    .unwrap()
    .with_tx(peripherals.GPIO15)
    .with_rx(peripherals.GPIO23)
    .into_async();

  // I2C
  let i2c_config = I2cConfig::default().with_frequency(Rate::from_khz(400));
  let i2c = I2c::new(peripherals.I2C0, i2c_config)
    .unwrap()
    .with_sda(peripherals.GPIO11)
    .with_scl(peripherals.GPIO10) 
    .into_async(); 
   let i2c_bus: &'static I2cBus = I2C_BUS.init(Mutex::new(i2c));

  // Wifi initializtion
  // Setup new mac
 
  esp_hal::efuse::override_mac_address(
    MacAddress::new_eui48([0x02_u8, 0x00, 0x00, 0x00, 0x00, NODE_ADDR as u8])
  ).unwrap();

  // -------- WiFi --------
  // Controller and IF
  let wifi_controller = esp_radio::wifi::WifiController::new(
    peripherals.WIFI,
    ControllerConfig::default(),
  )
  .unwrap();
  let wifi_controller: &'static mut WifiController<'static> =
    Box::leak(Box::new(wifi_controller));
  let esp_now = wifi_controller.esp_now();
  esp_now.set_channel(3).unwrap();
  
  // Run tasks
  spawner.spawn(aes_backend_task(peripherals.AES).unwrap());

  // Sync source
  let _ = NmeaSyncSource::builder()
    .pps_pin(peripherals.GPIO1.degrade())
    .spawner(&spawner)
    .uart(uart1)
    .build();

  // Power manager
  let power_manager = PowerManager::new(&i2c_bus);
  power_manager.confiure().await;

  // Burelom node
  let key: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
  let cipher = Esp32Cypher::new(key);
  let led_pin = peripherals.GPIO3.degrade();
  let mac = EspNowMac::new(esp_now, led_pin, &spawner);
  let rand = Esp32Rand::new();
  
  let node = BurelomNode::builder()
    .addr(NODE_ADDR)
    .roles(DeviceRole::DEFAULT | DeviceRole::BEACON)
    .mac(Box::new(mac))
    .cipher(Box::new(cipher))
    .hello_interval(Duration::from_secs(3))
    .route_ttl(Duration::from_secs(25))
    .rand(Box::new(rand))
    .beacon_interval(Duration::from_secs(30))
    .build().unwrap();
  node.run(&spawner);
  
  // Telemtry send loop
  loop {
    let solar_va = power_manager.get_solar_va()
      .await
      .unwrap();
    let charging_va = power_manager.get_charging_va()
      .await
      .unwrap();
    let battary_charge = power_manager.get_battary_charge()
      .await
      .unwrap();
    let device_consumtion = power_manager.get_device_consumtion()
      .await
      .unwrap();

    info!("\nDEVICE POWER STATE:\nSolar panel: {} W\nCharging: {} W\nBattary charge: {} %\nDevice conumtion: {} W\n----------", &solar_va, charging_va, battary_charge as i8, device_consumtion);

    let telemetry = proto::Telemetry {
      uptime: 0,
      solar_va,
      charging_va,
      battary_charge,
      device_consumtion,
    };
    let mut buf = Vec::new();
    telemetry.encode(&mut buf).unwrap();


    let _ = node.send(0x02, &buf).await;

    Timer::after_secs(5).await;
  }
}

#[embassy_executor::task]
async fn aes_backend_task(aes_peripheral: AES<'static>) {
  let mut backend = AesBackend::new(aes_peripheral);
  let _driver = backend.start();
  core::future::pending::<()>().await;
}