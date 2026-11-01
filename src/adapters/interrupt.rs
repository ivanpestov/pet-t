//! Прерывание по Ctrl+C.

use std::sync::mpsc::Sender;

use crate::application::event::Event;

/// Устанавливает обработчик Ctrl+C, отправляющий `Event::Interrupt` в
/// канал событий.
pub fn install(events: Sender<Event>) -> Result<(), String> {
    ctrlc::set_handler(move || {
        let _ = events.send(Event::Interrupt);
    })
    .map_err(|error| format!("cannot install Ctrl+C handler: {error}"))
}
