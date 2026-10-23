use std::time::Duration;
use eframe::egui;
use tokio::sync::mpsc;
use crate::commands::Message;
use crate::state::AppState;
use crate::{ui, update};

pub struct App {
  pub state: AppState,
  pub rx: mpsc::Receiver<Message>,
}

impl App {
  pub fn new(
    rx: mpsc::Receiver<Message>,
    paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
  ) -> Self {
    Self {
      state: AppState::new(paused),
      rx,
    }
  }
}

impl eframe::App for App {
  fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
    while let Ok(msg) = self.rx.try_recv() {
      update::handle(&mut self.state, msg);
    }
    
    ctx.request_repaint_after(Duration::from_millis(16));
  }
  
  fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
    ui::show(ui.ctx(), &mut self.state);
  }
}