use core::cell::RefCell;

use alloc::{boxed::Box, sync::Arc};
use defmt::info;
use embassy_executor::Spawner;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Instant, Timer};
use embassy_futures::select::{select, Either};
use esp_hal::gpio::{AnyPin, Level, Output, OutputConfig};

use crate::sync_source::SyncSource;

const NS_PER_SEC: u128 = 1_000_000_000;
const EPOCH_NS: u128 = 10 * NS_PER_SEC;     // 10 c
const WORK_OFFSET_NS: u128 = 0;              // начало эпохи
const OFF_OFFSET_NS: u128 = 2 * NS_PER_SEC; // 2 c от начала

enum Event { Work, Off }
struct Clock {
  base_instant: Instant,
  base_ns: u128,
}

impl Clock {
  fn new() -> Self {
    Self { base_instant: Instant::now(), base_ns: 0 }
  }
  
  fn sync(&mut self, secs: u64, nanos: u32) {
    self.base_instant = Instant::now();
    self.base_ns = secs as u128 * NS_PER_SEC + nanos as u128;
  }
  
  fn utc_ns(&self) -> u128 {
    self.base_ns + self.base_instant.elapsed().as_nanos() as u128
  }
  
  fn instant_at(&self, utc_ns: u128) -> Instant {
    let delta = utc_ns.saturating_sub(self.base_ns) as u64;
    self.base_instant + Duration::from_nanos(delta)
  }
}

pub fn new(led_pin: AnyPin<'static>, sync_source: Arc<dyn SyncSource>, spawner: &Spawner) {
  let sync_singnal: &'static Signal<CriticalSectionRawMutex, (u64, u32)> = 
  Box::leak(Box::new(Signal::new()));
  
  spawner.spawn(time_source_task(
    sync_source,
    sync_singnal
  ).unwrap());
  
  spawner.spawn(scheduler_task(
    led_pin,
    sync_singnal
  ).unwrap());
}

fn next_event_after(after_ns: u128) -> (u128, Event) {
  let epoch_start = (after_ns / EPOCH_NS) * EPOCH_NS;
  let work_at = epoch_start + WORK_OFFSET_NS;
  let off_at  = epoch_start + OFF_OFFSET_NS;
  
  if work_at > after_ns {
    (work_at, Event::Work)
  } else if off_at > after_ns {
    (off_at, Event::Off)
  } else {
    (work_at + EPOCH_NS, Event::Work)
  }
}

fn apply(led: &mut Output<'static>, event: &Event) {
  match event {
    Event::Work => { led.set_high(); info!("WORK"); }
    Event::Off  => { led.set_low();  info!("OFF");  }
  }
}


#[embassy_executor::task]
async fn time_source_task(
  sync_source: Arc<dyn SyncSource>,
  sync_singnal: &'static Signal<CriticalSectionRawMutex, (u64, u32)>
) {
  let mut sync_awaiter = sync_source.get_instance();
  
  loop {
    let (secs, nanos) = sync_awaiter.sync().await;
    sync_singnal.signal((secs, nanos));
  }
}

#[embassy_executor::task]
async fn scheduler_task(
  led_pin: AnyPin<'static>,
  sync_singnal: &'static Signal<CriticalSectionRawMutex, (u64, u32)>,
) {
  let mut clock = Clock::new();
  let mut led = Output::new(led_pin, Level::Low, OutputConfig::default());
  
  let (s, n) = sync_singnal.wait().await;
  clock.sync(s, n);
  info!("scheduler: synced");
  
  let mut last_ns = clock.utc_ns();
  
  loop {
    let (event_ns, event) = next_event_after(last_ns);
    let target = clock.instant_at(event_ns);
    
    match select(sync_singnal.wait(), Timer::at(target)).await {
      Either::First((s, n)) => {
        clock.sync(s, n);
        // Если по новым часам событие уже наступило — выполняем сейчас,
        // иначе просто перепланируем (last_ns не трогаем).
        if clock.utc_ns() >= event_ns {
          info!("Sync secs: {}, nanos {}:", s, n);
          apply(&mut led, &event);
          last_ns = event_ns;
        }
      }
      Either::Second(_) => {
        apply(&mut led, &event);
        last_ns = event_ns;
      }
    }
  }
}