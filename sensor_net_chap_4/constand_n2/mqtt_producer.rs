use core::{cell::RefCell};

use alloc::sync::Arc;
use defmt::{info, warn};
use embassy_executor::Spawner;
use embassy_futures::select::{Either, select};
use embassy_net::{IpAddress, Stack, tcp::TcpSocket};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal};
use embassy_time::{Duration, Timer, with_timeout};
use heapless::{Deque, String, Vec};
use rust_mqtt::{buffer::AllocBuffer, client::{Client, event::Event, options::{ConnectOptions, PublicationOptions, TopicReference}}, types::{MqttBinary, MqttString, PacketIdentifier, TopicName}};

const PAYLOAD_SIZE: usize = 128;
const CACHE_CAPACITY: usize = 32;
const TCP_BUFFER_SIZE: usize = 2048;

type Data = Vec<u8, PAYLOAD_SIZE>;
type Topic = String<32>;
type Queue = Arc<Mutex<CriticalSectionRawMutex,RefCell<Deque<(Data, Topic), CACHE_CAPACITY>>>>;
type QueuePushSignal = Arc<Signal<CriticalSectionRawMutex, ()>>;


pub struct MqttProducer {
  queue: Queue,
  next_data: QueuePushSignal
}

impl MqttProducer {
  pub fn new(
    spawner: &Spawner,
    stack: Stack<'static>,
    device_id: String<16>,
    broker_addr: IpAddress,
    broker_port: u16,
    broker_user: String<16>,
    broker_password: String<16>
  ) -> Self {
    let queue = Arc::new(Mutex::new(RefCell::new(Deque::new())));
    let next_data = Arc::new(Signal::new());

    spawner.spawn(mqtt_send_task(
      stack,
      device_id,
      broker_addr,
      broker_port,
      broker_user,
      broker_password,
      queue.clone(),
      next_data.clone()
    ).unwrap());

    MqttProducer { 
      queue,
      next_data
    }
  }

  pub async fn push(&self, data: Data, topic: Topic) {
    let guard = self.queue.lock().await;
    let mut queue = guard.borrow_mut();

    let overwritten = queue.is_full();
    
    if overwritten {
      let _ = queue.pop_front();
    }
    let _ = queue.push_back((data, topic));

    if overwritten {
      warn!("Data cache full: oldest queued message dropped");
    }
    
    self.next_data.signal(());
  }
}

async fn cache_pop(queue: &Queue,) -> Option<(Data, Topic)> {
  queue.lock().await.borrow_mut().pop_front()
}

async fn wait_for_message(
  queue: &Queue,
  next_data: &QueuePushSignal
) -> (Data, Topic) {
  loop {
    if let Some(message) = cache_pop(queue).await {
      return message;
    }
    
    next_data.wait().await;
  }
}

#[embassy_executor::task]
pub async fn mqtt_send_task(
  stack: Stack<'static>,
  device_id: String<16>,
  broker_addr: IpAddress,
  broker_port: u16,
  broker_user: String<16>,
  broker_password: String<16>,
  queue: Queue,
  next_data: QueuePushSignal
) {
  let mut rx_buffer = [0u8; TCP_BUFFER_SIZE];
  let mut tx_buffer = [0u8; TCP_BUFFER_SIZE];
  let mut pending: Option<(Data, Topic)> = None;
  
  loop {
    stack.wait_config_up().await;
    
    info!("NEW");
    // --- SOCKET ---
    let mut socket = TcpSocket::new(
      stack,
      &mut rx_buffer,
      &mut tx_buffer,
    );
    
    socket.set_timeout(Some(Duration::from_secs(10)));
    
    let endpoint = (
      broker_addr,
      broker_port,
    );
    
    match with_timeout(
      Duration::from_secs(10),
      socket.connect(endpoint))
      .await {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
          warn!("TCP connect failed: {:?}", e);
          Timer::after_secs(5).await;
          continue;
        }
        Err(_) => {
          warn!("TCP connect timeout");
          Timer::after_secs(5).await;
          continue;
        }
      }
      
      // --- MQTT CLIENT --- 
      let mut buffer = AllocBuffer;
      let mut client: Client<
        '_,
        '_,
        TcpSocket<'_>,
        AllocBuffer,
        1,
        1,
        1,
        1,
        1,
        1,
        1,
      > = Client::new(&mut buffer);
      
      let connect_options = ConnectOptions::new()
        .clean_start()
        .user_name(MqttString::from_str(&broker_user).unwrap())
        .password(MqttBinary::from_slice(broker_password.as_bytes()).unwrap());
      let device_id = Some(MqttString::from_str(&device_id).unwrap());
      
      match with_timeout(
        Duration::from_secs(10),
        client.connect(socket,&connect_options, device_id )
      )
      .await
      {
        Ok(Ok(_)) => {
          info!("MQTT connected");
        }
        Ok(Err(e)) => {
          warn!("MQTT connect failed: {:?}", e);
          Timer::after_secs(5).await;
          continue;
        }
        Err(_) => {
          warn!("MQTT connect timeout");
          Timer::after_secs(5).await;
          continue;
        }
      }
      
      // --- PUBLICATION AND POLL LOOP
      let mut in_flight: Option<PacketIdentifier> = None;
      loop {
        let data_fut = async {
          if in_flight.is_some() {
              core::future::pending::<(Data, Topic)>().await
          } else {
            match pending.take() {
              Some(t) => t,
              None => wait_for_message(&queue, &next_data).await,
            }
          }
        };

        let race_fut = select(
          data_fut,
          client.poll_header()
        );
        
        let timeout_fut 
          = with_timeout(Duration::from_secs(5), race_fut).await;
        
        match timeout_fut {
          Ok(Either::First((data, topic))) => {
            pending = Some((data.clone(), topic.clone()));
            let topic = MqttString::from_str(&topic).unwrap();
            let topic_reference = TopicReference::Name(TopicName::new(topic).unwrap());

            let publication_options = PublicationOptions::new(topic_reference.as_borrowed())
            .exactly_once();
            
            let publish_fut = client.publish(
              &publication_options,
              (data.as_slice()).into(),
            );
            
            let timeout_fut 
            = with_timeout(Duration::from_secs(10), publish_fut).await;
            
            match timeout_fut {
              Ok(Ok(Some(pid))) => {
                in_flight = Some(pid);
              },
              Ok(Ok(None)) => {
                warn!("MQTT publication rejected");
              },
              Ok(Err(e)) => {
                warn!("MQTT publish failed: {:?}", e);
                Timer::after_secs(5).await;
                break;
              },
              Err(_) => {
                warn!("MQTT publication timeout");
                Timer::after_secs(5).await;
                break;
              }
            }
          }
          Ok(Either::Second(header_result)) => {
            match header_result {
              Ok(header) => {
                match client.poll_body(header).await {
                  Ok(Event::PublishComplete(pid)) => {
                    if in_flight.is_some() {
                      if pid.packet_identifier == in_flight.unwrap() {
                        in_flight = None;
                        pending = None;
                      }
                    }
                  }
                  Ok(Event::PublishRejected(_)) => {
                    if in_flight.is_some() {
                      in_flight = None;
                      pending = None;
                    }
                  }
                  Ok (event) => {
                    info!("MQTT event: {:?}", event);
                  } 
                  Err(error) => {
                    warn!("Failed to process MQTT packet: {:?}", error);
                  }
                }
              },
              Err(error) => {
                warn!("Failed to process MQTT packet: {:?}", error);
              }
            }
          }
          Err(_) => {
            let _ = client.ping().await;
          }
        }
      }
    }
  }
