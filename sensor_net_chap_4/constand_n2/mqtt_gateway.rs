use core::future::pending;
use core::fmt::Write;
use alloc::{sync::Arc, vec::Vec};
use alloc::boxed::Box;
use async_trait::async_trait;
use bon::{bon};
use burelom_net::traits::gateway::Gateway;
use embassy_executor::Spawner;
use embassy_net::{IpAddress, Stack};
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex,};
use embassy_sync::mutex::Mutex;
use crate::mqtt_producer::{MqttProducer};

type MqttProducerHandler = Arc<Mutex<CriticalSectionRawMutex, MqttProducer>>;

pub struct MqttGateway {
  mqtt_producer: MqttProducerHandler
}

#[bon]
impl MqttGateway {
  #[builder]
  pub fn new(
    stack: Stack<'static>,
    spawner: &Spawner,
    node_addr: u32,
    broker_addr: IpAddress,
    broker_port: u16,
    broker_user: &str,
    broker_password: &str
) -> Self {
    let mut device_id = heapless::String::<16>::new();
    write!(&mut device_id, "{}", node_addr).unwrap();
    let broker_user = heapless::String::try_from(broker_user).unwrap();
    let broker_password = heapless::String::try_from(broker_password).unwrap();

    let telemetry_producer = MqttProducer::new(
      &spawner,
      stack,
      device_id,
      broker_addr,
      broker_port,
      broker_user,
      broker_password
    );

    MqttGateway { 
      mqtt_producer: Arc::new(Mutex::new(telemetry_producer))
    }
  }
}

#[async_trait]
impl Gateway for MqttGateway {
  async fn send(&self, source: u32, data: &Vec<u8>) {
    let mqtt_producer = self.mqtt_producer.lock().await;

    let data: heapless::Vec<u8, 128> = heapless::Vec::from_slice(&data).unwrap();
    let mut topic = heapless::String::<32>::new();
    write!(&mut topic, "{}", source).unwrap();
    
    mqtt_producer.push(data, topic).await;
  }

  async fn recv(&self) -> (u32, Vec<u8>) {
    pending().await
  }
}