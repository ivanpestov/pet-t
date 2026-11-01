//! События, поступающие в цикл Таймера.

use std::sync::mpsc::Sender;

/// Событие цикла Таймера. Канал остаётся границей наблюдения за сном;
/// отдельный трейт наблюдения не вводится.
pub enum Event {
    Interrupt,
    Suspend(Sender<()>),
    MonitorFailed(String),
}
