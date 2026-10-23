use std::sync::atomic::AtomicBool;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::mpsc;

mod app;
mod background;
mod commands;
mod state;
mod ui;
mod update;
mod proto;

static TOKIO_HANDLE: OnceLock<Handle> = OnceLock::new();

#[allow(dead_code)]
pub fn tokio_handle() -> &'static Handle {
  TOKIO_HANDLE.get().expect("Tokio runtime is not initialized")
}

fn main() -> eframe::Result {
  tracing_subscriber::fmt()
    .with_env_filter("info")
    .init();

  let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
  let handle = rt.handle().clone();
  let _ = TOKIO_HANDLE.set(handle);
  
  std::thread::spawn(move || {
    rt.block_on(async {
      loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
      }
    });
  });
  
  let (tx, rx) = mpsc::channel::<commands::Message>(1024);
  let paused = Arc::new(AtomicBool::new(false));
  
  background::poller::spawn(tx, paused.clone());
  
  let options = eframe::NativeOptions {
    viewport: eframe::egui::ViewportBuilder::default()
    .with_inner_size([1000.0, 700.0])
    .with_resizable(true),
    ..Default::default()
  };
  
  eframe::run_native(
    "Constand telemetry",
    options,
    Box::new(move |_cc| Ok(Box::new(app::App::new(rx, paused)))),
  )
}




