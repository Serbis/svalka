use alloc::boxed::Box;
use bon::bon;
use defmt::info;
use embassy_executor::Spawner;
use embassy_sync::{ blocking_mutex::raw::CriticalSectionRawMutex, watch::Watch} ;
use esp_radio::esp_now::{ EspNowReceiver };
use crate::sync_source::{ SyncAwaiter, SyncSource };

const SYNC_CONSUMERS: usize = 5;

pub struct EspNowSyncSource {
  sync_watch: &'static Watch<CriticalSectionRawMutex, (u64, u32), SYNC_CONSUMERS>
}

#[bon]
impl EspNowSyncSource {
  #[builder]
  pub fn new(
    esp_now_receiver: EspNowReceiver,
    spawner: &Spawner
  ) -> Self {
    let sync_watch: &'static Watch<_, _, SYNC_CONSUMERS> = Box::leak(Box::new(Watch::new()));
    
    spawner.spawn(esp_now_task(
      esp_now_receiver,
      sync_watch
    ).unwrap());
    
    EspNowSyncSource { 
      sync_watch
    }
  }
}

impl SyncSource for EspNowSyncSource {
  fn get_instance(&self) -> SyncAwaiter {
    let receiver = self.sync_watch.receiver().unwrap();
    SyncAwaiter::new(receiver)
  } 
}

#[embassy_executor::task]
async fn esp_now_task(
  mut esp_now_receiver: EspNowReceiver,
  sync_watch: &'static Watch<CriticalSectionRawMutex, (u64, u32), SYNC_CONSUMERS>
) {
  loop {
    info!("Wait sync from esp_now");
    let received = esp_now_receiver
      .receive_async()
      .await;
     info!("Received sync from esp_now");
    let buffer = received.data();

    let secs = u64::from_le_bytes(
        buffer[0..8].try_into().unwrap()
    );
    let nanos = u32::from_le_bytes(
        buffer[8..12].try_into().unwrap()
    );
    
    sync_watch.sender().send((secs, nanos));
  }
}