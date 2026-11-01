//! Уведомления через `notify-send`; при неудаче — звонок и заголовок в
//! поток диагностики.

use std::io::Write;
use std::process::Command;

use crate::application::ports::Notifier;

/// Notifier на основе команды `notify-send`. Stderr дочернего процесса
/// остаётся унаследованным; fallback выполняется здесь, а не в сценарии.
pub struct NotifySend;

impl Notifier for NotifySend {
    fn notify(&self, title: &str, message: &str, diagnostics: &mut dyn Write) {
        let sent = Command::new("notify-send")
            .args([title, message])
            .status()
            .is_ok_and(|status| status.success());
        if !sent {
            let _ = writeln!(diagnostics, "\x07{title}");
        }
    }
}
