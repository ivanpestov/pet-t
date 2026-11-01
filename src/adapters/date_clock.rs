//! Часы через системную команду `date`.

use std::process::Command;

use crate::application::ports::{Clock, ClockError};
use crate::domain::time::{MinuteOfDay, Timestamp};

/// Часы на основе системной команды `date` с прежними аргументами и
/// прежними сообщениями об ошибках.
pub struct DateClock;

impl Clock for DateClock {
    fn local_minutes(&self) -> Result<MinuteOfDay, ClockError> {
        let output = Command::new("date")
            .arg("+%H:%M")
            .output()
            .map_err(|error| ClockError::new(format!("cannot run date: {error}")))?;
        if !output.status.success() {
            return Err(ClockError::new(format!(
                "date exited with {}",
                output.status
            )));
        }
        let time = String::from_utf8(output.stdout)
            .map_err(|error| ClockError::new(format!("date returned invalid UTF-8: {error}")))?;
        let time = time.trim();
        MinuteOfDay::parse(time)
            .map_err(|_| ClockError::new(format!("date returned invalid local time: {time}")))
    }

    fn ledger_timestamp(&self) -> Result<Timestamp, ClockError> {
        let output = Command::new("date")
            .arg("+%Y/%m/%d %H:%M:%S")
            .output()
            .map_err(|error| ClockError::new(format!("cannot run date: {error}")))?;
        if !output.status.success() {
            return Err(ClockError::new(format!(
                "date exited with {}",
                output.status
            )));
        }
        String::from_utf8(output.stdout)
            .map(|timestamp| Timestamp::new(&timestamp))
            .map_err(|error| ClockError::new(format!("date returned invalid UTF-8: {error}")))
    }
}
