use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Receiver};

pub struct SyncAwaiter {
  sync_receiver: Receiver<'static, CriticalSectionRawMutex, (u64, u32), 5>  
}

impl SyncAwaiter {
  pub fn new(sync_receiver: Receiver<'static, CriticalSectionRawMutex, (u64, u32), 5>) -> Self {
    SyncAwaiter { sync_receiver }
  }
   pub async fn sync(&mut self) -> (u64, u32) {
    self.sync_receiver.changed().await
   }
}

pub trait SyncSource: Send + Sync {
  fn get_instance(&self) -> SyncAwaiter;
}