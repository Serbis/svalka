use core::cell::RefCell;
use alloc::{boxed::Box, sync::Arc};
use bon::bon;
use defmt::{error};
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::{Mutex, raw::CriticalSectionRawMutex}, watch::Watch};
use embassy_time::Instant;
use esp_hal::{Async, gpio::{AnyPin, Input, InputConfig, Pull}, uart::Uart};
use nmea::ParseResult;
use chrono::{Timelike};
use crate::sync_source::{SyncAwaiter, SyncSource};

const SYNC_CONSUMERS: usize = 5;

pub struct NmeaSyncSource {
  sync_watch: &'static Watch<CriticalSectionRawMutex, (u64, u32), SYNC_CONSUMERS>
}

#[bon]
impl NmeaSyncSource {
  #[builder]
  pub fn new(
    pps_pin: AnyPin<'static>,
    uart: Uart<'static, Async>,
    spawner: &Spawner
  ) -> Self { 
    let time_base = Arc::new(Mutex::new(RefCell::new(None)));
    let sync_watch: &'static Watch<_, _, SYNC_CONSUMERS> =Box::leak(Box::new(Watch::new()));
    
    spawner.spawn(pps_task(pps_pin, time_base.clone()).unwrap());
    spawner.spawn(nmea_task(uart, sync_watch, time_base.clone()).unwrap());
        
    NmeaSyncSource { sync_watch }
  }
}

impl SyncSource for NmeaSyncSource {
  fn get_instance(&self) -> SyncAwaiter {
    let receiver = self.sync_watch.receiver().unwrap();
    SyncAwaiter::new(receiver)
  } 
}

struct TimeBase {
  instant: Option<Instant>,   
  utc_secs: u64,      
  utc_nanos: u32,  
}

fn now_utc(time_base: &TimeBase) -> Option<(u64, u32, u64)> {
  if time_base.utc_secs == 0 || time_base.instant.is_none() {
    return None;
  }
  
  let dt_nanos = Instant::now().as_nanos() - time_base.instant.as_ref().unwrap().as_nanos();
  
  let mut secs = time_base.utc_secs;
  let mut nanos = time_base.utc_nanos as u64 + dt_nanos;
  
  secs += (nanos / 1_000_000_000) as u64;
  nanos %= 1_000_000_000;
  
  Some((secs, nanos as u32, dt_nanos))
}

#[embassy_executor::task]
async fn pps_task(
  pps_pin: AnyPin<'static>,
  time_base: Arc<Mutex<CriticalSectionRawMutex, RefCell<Option<TimeBase>>>>
) {
  let mut pps_pin = Input::new(pps_pin, InputConfig::default().with_pull(Pull::Down));
  
  loop {
    pps_pin.wait_for_rising_edge().await;
    
    time_base.lock(|guard| {
      let mut tb = guard.borrow_mut();

      if tb.as_ref().is_some() {
        let tb = tb.as_mut().unwrap();
        tb.instant = Some(Instant::now());
      }
    });
  }
}

#[embassy_executor::task]
async fn nmea_task(
  uart: Uart<'static, Async>,
  sync_watch: &'static Watch<CriticalSectionRawMutex, (u64, u32), SYNC_CONSUMERS>,
  time_base: Arc<Mutex<CriticalSectionRawMutex, RefCell<Option<TimeBase>>>>,
  
) {
  let (mut rx, mut tx) = uart.split();
  let config_cmd = "$PCAS03,1,0,0,0,0,0,1,0*02\r\n";
  let _ = tx.write_async(config_cmd.as_bytes()).await;
  

  let mut line_buf = [0u8; 128];
  let mut line_len = 0;
  let mut rx_buf = [0u8; 64];
  
  loop {
    let n = match rx.read_async(&mut rx_buf).await {
      Ok(0) => continue,
      Ok(n) => n,
      Err(e) => {
        error!("UART RX error: {:?}", e);
        continue;
      }
    };
    
    for &b in &rx_buf[..n] {
      match b {
        b'\n' => {
          if line_len > 0 {
            let end = if line_buf[line_len - 1] == b'\r' {
              line_len - 1
            } else {
              line_len
            };
            
            if let Ok(s) = core::str::from_utf8(&line_buf[..end]) {
              if s.starts_with('$') {
                //info!("{}", s);
                match nmea::parse_str(s) {
                  Ok(ParseResult::ZDA(data)) => { 
                    let utc_date_time = data.utc_date_time();
                    //info!("Parcse NMEA: {:?}", &data);
                    if utc_date_time.is_none() {
                      //info!("No data");
                      continue;
                    }
                    let time = utc_date_time.unwrap();
                    let secs = time.and_utc().timestamp();
                    let nanos = time.nanosecond();
                    
                    time_base.lock(|gaurd| {
                      let mut time_base = gaurd.borrow_mut();
                      
                      if time_base.is_some() {
                        let time_base_unw = time_base.as_mut().unwrap();
                        time_base_unw.utc_secs = secs as u64;
                        time_base_unw.utc_nanos = nanos;
                      } else {
                        time_base.replace(TimeBase {
                          instant: None,
                          utc_nanos: nanos,
                          utc_secs: secs as u64,
                        });
                      }

                      if let Some((secs, nanos, _)) = now_utc(time_base.as_ref().unwrap()) {
                        sync_watch.sender().send((secs as u64, nanos));
                      }
                    }); 
                  }
                  Ok(ParseResult::GGA(data)) => { 
                    //info!("Fix Type: {:?}", data.fix_type);
                  }
                  Ok(_) => {}
                  Err(e) => {
                    error!("NMEA parse error on {:?}: {}", s, e);
                  }
                }
              }
            }
          }
          line_len = 0;
        }
        b'\r' => {}
        _ => {
          if line_len < 128 - 1 {
            line_buf[line_len] = b;
            line_len += 1;
          } else {
            defmt::warn!("NMEA line too long, dropping");
            line_len = 0;
          }
        }
      }
    }
  }
}

