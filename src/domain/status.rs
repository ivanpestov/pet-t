//! Состояние Таймера: чистое формирование строки статуса.

use std::time::Duration;

use crate::domain::time::MinuteOfDay;

/// Состояние Таймера: время старта, остаток и время завершения. Формирует
/// строку статуса; вывод в поток и сброс буфера выполняет вызывающий код.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerStatus {
    started_at: MinuteOfDay,
    remaining: Duration,
    finishes_at: MinuteOfDay,
}

impl TimerStatus {
    pub fn new(started_at: MinuteOfDay, remaining: Duration, finishes_at: MinuteOfDay) -> Self {
        Self {
            started_at,
            remaining,
            finishes_at,
        }
    }

    pub fn render(&self) -> String {
        let seconds = self.remaining.as_secs() + u64::from(self.remaining.subsec_nanos() != 0);
        let remaining_minutes = seconds.div_ceil(60);
        format!(
            "\r{} => {:02}:{:02} => {}",
            self.started_at,
            remaining_minutes / 60,
            remaining_minutes % 60,
            self.finishes_at
        )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::TimerStatus;
    use crate::domain::time::MinuteOfDay;

    fn time(hours: u64, minutes: u64) -> MinuteOfDay {
        MinuteOfDay::from_hms(hours, minutes).expect("valid local time")
    }

    #[test]
    fn renders_start_remaining_and_finish_separated_by_arrows() {
        let status = TimerStatus::new(time(12, 34), Duration::from_secs(90 * 60), time(14, 4));
        assert_eq!(status.render(), "\r12:34 => 01:30 => 14:04");
    }

    #[test]
    fn rounds_the_remaining_time_up_to_whole_minutes() {
        let cases = [
            (Duration::from_secs(0), "00:00"),
            (Duration::from_secs(1), "00:01"),
            (Duration::from_secs(59), "00:01"),
            (Duration::from_secs(60), "00:01"),
            (Duration::from_secs(61), "00:02"),
            (Duration::from_millis(119_500), "00:02"),
            (Duration::from_secs(120), "00:02"),
            (Duration::from_secs(90 * 60), "01:30"),
            (Duration::from_secs(25 * 3600), "25:00"),
        ];
        for (remaining, expected) in cases {
            let status = TimerStatus::new(time(12, 34), remaining, time(14, 4));
            assert_eq!(status.render(), format!("\r12:34 => {expected} => 14:04"));
        }
    }

    #[test]
    fn allows_zero_remaining_in_a_final_display() {
        let status = TimerStatus::new(time(12, 34), Duration::ZERO, time(12, 36));
        assert_eq!(status.render(), "\r12:34 => 00:00 => 12:36");
    }
}
