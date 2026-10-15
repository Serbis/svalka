use core::net::SocketAddr;
use alloc::{boxed::Box, string::String};
use bon::bon;
use defmt::error;
use embassy_executor::Spawner;
use embassy_net::{Stack, dns::DnsQueryType, udp::{PacketMetadata, UdpSocket}};
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, watch::Watch};
use embassy_time::{Duration, Timer, with_timeout};
use sntpc::NtpContext;
use sntpc_net_embassy::UdpSocketWrapper;
use sntpc_time_embassy::EmbassyTimestampGenerator;
use crate::sync_source::{SyncAwaiter, SyncSource};

const SYNC_CONSUMERS: usize = 5;

pub struct NtpSyncSource {
  sync_watch: &'static Watch<CriticalSectionRawMutex, (u64, u32), SYNC_CONSUMERS>
}

#[bon]
impl NtpSyncSource {
  #[builder]
  pub fn new(
    ntp_server: String,
    ntp_port: u16,
    stack: Stack<'static>,
    recall_interval: Duration,
    spawner: &Spawner
  ) -> Self {
    let sync_watch: &'static Watch<_, _, SYNC_CONSUMERS> =Box::leak(Box::new(Watch::new()));
    
    spawner.spawn(ntp_task(
      ntp_server,
      ntp_port,
      stack,
      recall_interval,
      sync_watch
    ).unwrap());
    
    NtpSyncSource { 
      sync_watch
    }
  }
}

impl SyncSource for NtpSyncSource {
  fn get_instance(&self) -> SyncAwaiter {
    let receiver = self.sync_watch.receiver().unwrap();
    SyncAwaiter::new(receiver)
  } 
}

#[embassy_executor::task]
async fn ntp_task(
  ntp_server: String,
  ntp_port: u16,
  stack: Stack<'static>,
  recall_interval: Duration,
  sync_watch: &'static Watch<CriticalSectionRawMutex, (u64, u32), SYNC_CONSUMERS>
) {
  loop {
    let dns_resolve = stack
    .dns_query(&ntp_server, DnsQueryType::A)
    .await;
    if dns_resolve.is_err() {
      error!("NTP server dns resolve error: {}", dns_resolve.err().unwrap());
      Timer::after(recall_interval).await;
      continue;
    }
    
    let dns_entry = dns_resolve
    .ok()
    .unwrap();
    
    if dns_entry.first().is_none() {
      error!("NTP server dns entry not found");
      Timer::after(recall_interval).await;
      continue;
    }
    let addr = *dns_entry.first().unwrap();
    
    let remote_endpoint = SocketAddr::new(addr.into(), ntp_port);
    
    let mut rx_meta = [PacketMetadata::EMPTY; 16];
    let mut rx_buffer = [0u8; 4096];
    let mut tx_meta = [PacketMetadata::EMPTY; 16];
    let mut tx_buffer = [0u8; 4096];
    
    let mut socket = UdpSocket::new(
      stack,
      &mut rx_meta,
      &mut rx_buffer,
      &mut tx_meta,
      &mut tx_buffer,
    );
    let _ = socket.bind(0).map_err(|_| sntpc::Error::Network);
    
    let socket = UdpSocketWrapper::from(socket);
    let ntp_context = NtpContext::new(EmbassyTimestampGenerator::default());
    
    match with_timeout(
      Duration::from_secs(3),
       sntpc::get_time(remote_endpoint, &socket, ntp_context)
      ).await {
        Ok(Ok(time)) => {
          let sec = time.sec() as u64;
          let frac = (time.sec_fraction() as u128 * 1_000_000_000 / (1u128 << 32)) as u32;
          sync_watch.sender().send((sec, frac));
        }
        Ok(Err(e)) => {
          error!("SNTP request failed: {}", e);
        }
        Err(_) => {
          error!("SNTP request timed out after 3 secs");
        }
      }
    
    Timer::after(recall_interval).await;
  }
}