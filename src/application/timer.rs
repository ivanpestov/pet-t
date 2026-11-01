//! Сценарий прогона Таймера: цикл событий и жизненный цикл Учитываемого
//! периода.

use std::io::{self, Write};
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use crate::application::config::Config;
use crate::application::event::Event;
use crate::application::outcome::Outcome;
use crate::application::ports::{Clock, LedgerWriter, Notifier};
use crate::domain::status::TimerStatus;
use crate::domain::time::MinuteOfDay;

const DISPLAY_INTERVAL: Duration = Duration::from_secs(60);
const DAY_MINUTES: u64 = 24 * 60;

/// Запускает Таймер: открывает Учитываемый период, выводит начальное
/// состояние и крутит цикл событий до естественного завершения, прерывания,
/// сна или отказа. Состояние пишет в `state` (после каждой записи —
/// сброс буфера), диагностику — в `diagnostics`. Исход прогона возвращает
/// как `Outcome`; перевод исхода в код выхода выполняет бинарный крейт.
pub fn run(
    config: &Config,
    clock: &dyn Clock,
    mut ledger: Option<&mut dyn LedgerWriter>,
    notifier: &dyn Notifier,
    events: &Receiver<Event>,
    state: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Outcome {
    if let (Some(account), Some(writer)) = (&config.account, ledger.as_mut())
        && let Err(error) = writer.open_period(account)
    {
        let _ = writeln!(diagnostics, "{error}");
        return Outcome::Failed;
    }

    let start = Instant::now();
    let deadline = start + config.duration;
    let mut next_display = start + DISPLAY_INTERVAL;
    let started_at = match clock.local_minutes() {
        Ok(started_at) => started_at,
        Err(error) => {
            let _ = writeln!(diagnostics, "cannot display timer: {error}");
            let _ = close_ledger(&mut ledger, diagnostics);
            return Outcome::Failed;
        }
    };
    let planned_finish_at = planned_finish(started_at, config.duration);

    if let Err(error) = display_status(started_at, planned_finish_at, config.duration, state) {
        let _ = writeln!(diagnostics, "cannot display timer: {error}");
        let _ = close_ledger(&mut ledger, diagnostics);
        return Outcome::Failed;
    }

    loop {
        let now = Instant::now();
        if now >= deadline {
            return finish_naturally(started_at, ledger, notifier, clock, state, diagnostics);
        }

        let wake_at = next_display.min(deadline);
        let received = events.recv_timeout(wake_at.saturating_duration_since(now));
        let now = Instant::now();
        if now >= deadline {
            return finish_naturally(started_at, ledger, notifier, clock, state, diagnostics);
        }
        let remaining = deadline.saturating_duration_since(now);

        match received {
            Ok(Event::Interrupt) => {
                let display_result = display_finished_status(started_at, remaining, clock, state);
                let close_result = close_ledger(&mut ledger, diagnostics);
                if let Err(error) = &display_result {
                    let _ = writeln!(diagnostics, "cannot display timer: {error}");
                }
                return Outcome::Interrupted {
                    closed: close_result.is_ok(),
                };
            }
            Ok(Event::Suspend(release)) => {
                let display_result = display_finished_status(started_at, remaining, clock, state);
                let close_result = close_ledger(&mut ledger, diagnostics);
                let _ = release.send(());
                if let Err(error) = &display_result {
                    let _ = writeln!(diagnostics, "cannot display timer: {error}");
                }
                let _ = writeln!(diagnostics, "System suspend detected; timer stopped");
                return Outcome::Suspended {
                    closed: close_result.is_ok(),
                };
            }
            Ok(Event::MonitorFailed(error)) => {
                if let Err(display_error) =
                    display_finished_status(started_at, remaining, clock, state)
                {
                    let _ = writeln!(diagnostics, "cannot display timer: {display_error}");
                }
                let _ = writeln!(diagnostics, "system sleep monitor failed: {error}");
                let _ = close_ledger(&mut ledger, diagnostics);
                return Outcome::Failed;
            }
            Err(RecvTimeoutError::Timeout) => {
                if now >= next_display {
                    if let Err(error) =
                        display_status(started_at, planned_finish_at, remaining, state)
                    {
                        let _ = writeln!(diagnostics, "cannot display timer: {error}");
                        let _ = close_ledger(&mut ledger, diagnostics);
                        return Outcome::Failed;
                    }
                    while next_display <= now {
                        next_display += DISPLAY_INTERVAL;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if let Err(error) = display_finished_status(started_at, remaining, clock, state) {
                    let _ = writeln!(diagnostics, "cannot display timer: {error}");
                }
                let _ = writeln!(diagnostics, "timer event channel disconnected");
                let _ = close_ledger(&mut ledger, diagnostics);
                return Outcome::Failed;
            }
        }
    }
}

/// Закрывает Учитываемый период, если он открывался; ошибку пишет в
/// диагностику.
fn close_ledger(
    ledger: &mut Option<&mut dyn LedgerWriter>,
    diagnostics: &mut dyn Write,
) -> Result<(), ()> {
    match ledger
        .as_mut()
        .map(|writer| writer.close_period())
        .transpose()
    {
        Ok(_) => Ok(()),
        Err(error) => {
            let _ = writeln!(diagnostics, "{error}");
            Err(())
        }
    }
}

/// Выводит строку состояния и сбрасывает буфер.
fn display_status(
    started_at: MinuteOfDay,
    finishes_at: MinuteOfDay,
    remaining: Duration,
    state: &mut dyn Write,
) -> io::Result<()> {
    let status = TimerStatus::new(started_at, remaining, finishes_at);
    write!(state, "{}", status.render())?;
    state.flush()
}

/// Выводит итоговое состояние с фактическим временем завершения и
/// завершает его переводом строки.
fn display_finished_status(
    started_at: MinuteOfDay,
    remaining: Duration,
    clock: &dyn Clock,
    state: &mut dyn Write,
) -> Result<(), String> {
    let finished_at = clock.local_minutes().map_err(|error| error.to_string())?;
    display_status(started_at, finished_at, remaining, state).map_err(|error| error.to_string())?;
    state
        .write_all(b"\n")
        .and_then(|()| state.flush())
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Планируемое время завершения: время старта плюс длительность с
/// усечением до целых минут, по модулю суток.
fn planned_finish(started_at: MinuteOfDay, duration: Duration) -> MinuteOfDay {
    let minutes = (u64::from(started_at.minutes()) + duration.as_secs() / 60) % DAY_MINUTES;
    MinuteOfDay::from_hms(minutes / 60, minutes % 60).expect("minutes stay within a day")
}

/// Естественное завершение: итоговое отображение, закрытие периода,
/// уведомление независимо от результата закрытия, затем отчёт об ошибке
/// отображения.
fn finish_naturally(
    started_at: MinuteOfDay,
    mut ledger: Option<&mut dyn LedgerWriter>,
    notifier: &dyn Notifier,
    clock: &dyn Clock,
    state: &mut dyn Write,
    diagnostics: &mut dyn Write,
) -> Outcome {
    let display_result = display_finished_status(started_at, Duration::ZERO, clock, state);
    let close_result = close_ledger(&mut ledger, diagnostics);
    notifier.notify("Timer finished", "Time is up", diagnostics);
    let displayed = display_result.is_ok();
    let closed = close_result.is_ok();
    if let Err(error) = display_result {
        let _ = writeln!(diagnostics, "cannot display timer: {error}");
    }
    Outcome::Completed { displayed, closed }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::sync::mpsc;
    use std::sync::{Mutex, MutexGuard};
    use std::thread;

    use super::*;
    use crate::application::ports::{ClockError, LedgerError};
    use crate::domain::account::Account;
    use crate::domain::time::Timestamp;

    const TITLE: &str = "Timer finished";
    const MESSAGE: &str = "Time is up";

    fn time(hours: u64, minutes: u64) -> MinuteOfDay {
        MinuteOfDay::from_hms(hours, minutes).expect("valid local time")
    }

    fn account() -> Account {
        Account::new("work").expect("valid account")
    }

    fn config(duration: Duration, with_account: bool) -> Config {
        Config {
            account: with_account.then(account),
            duration,
        }
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).expect("valid UTF-8")
    }

    fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
        mutex.lock().expect("unpoisoned mutex")
    }

    /// Часы с фиксированной очередью ответов: каждое чтение забирает
    /// следующий ответ, пустая очередь в конце теста подтверждает ровно
    /// ожидаемое число чтений.
    struct FakeClock {
        local_minutes: Mutex<Vec<Result<MinuteOfDay, ClockError>>>,
    }

    impl FakeClock {
        fn local(times: Vec<Result<MinuteOfDay, ClockError>>) -> Self {
            Self {
                local_minutes: Mutex::new(times),
            }
        }
    }

    impl Clock for FakeClock {
        fn local_minutes(&self) -> Result<MinuteOfDay, ClockError> {
            lock(&self.local_minutes).remove(0)
        }

        fn ledger_timestamp(&self) -> Result<Timestamp, ClockError> {
            panic!("the scenario never reads ledger timestamps")
        }
    }

    /// Хранилище с настраиваемыми результатами открытия и закрытия и
    /// журналом вызовов для проверки порядка.
    struct FakeLedger {
        open_result: Result<(), LedgerError>,
        close_results: Mutex<Vec<Result<(), LedgerError>>>,
        opened_with: Mutex<Vec<String>>,
        closes: Mutex<usize>,
        events: Mutex<Vec<String>>,
    }

    impl FakeLedger {
        fn working() -> Self {
            Self {
                open_result: Ok(()),
                close_results: Mutex::new(Vec::new()),
                opened_with: Mutex::new(Vec::new()),
                closes: Mutex::new(0),
                events: Mutex::new(Vec::new()),
            }
        }

        fn failing_to_close(message: &str) -> Self {
            let mut ledger = Self::working();
            ledger.close_results = Mutex::new(vec![Err(LedgerError::new(message))]);
            ledger
        }

        fn record(&self, event: &str) {
            lock(&self.events).push(event.to_owned());
        }
    }

    impl LedgerWriter for FakeLedger {
        fn open_period(&mut self, account: &Account) -> Result<(), LedgerError> {
            lock(&self.opened_with).push(account.as_str().to_owned());
            self.record("open");
            self.open_result.clone()
        }

        fn close_period(&mut self) -> Result<(), LedgerError> {
            *lock(&self.closes) += 1;
            self.record("close");
            let mut results = lock(&self.close_results);
            if results.is_empty() {
                Ok(())
            } else {
                results.remove(0)
            }
        }
    }

    #[derive(Default)]
    struct FakeNotifier {
        calls: Mutex<Vec<(String, String)>>,
    }

    impl FakeNotifier {
        fn calls(&self) -> Vec<(String, String)> {
            lock(&self.calls).clone()
        }
    }

    impl Notifier for FakeNotifier {
        fn notify(&self, title: &str, message: &str, _diagnostics: &mut dyn Write) {
            lock(&self.calls).push((title.to_owned(), message.to_owned()));
        }
    }

    /// Notifier, повторяющий fallback реального адаптера: звонок и
    /// заголовок в поток диагностики.
    struct BellNotifier;

    impl Notifier for BellNotifier {
        fn notify(&self, title: &str, _message: &str, diagnostics: &mut dyn Write) {
            let _ = writeln!(diagnostics, "\x07{title}");
        }
    }

    /// Поток состояния, падающий на записи с заданным порядковым номером.
    struct FailOnWrite {
        calls: Cell<usize>,
        fail_at: usize,
    }

    impl FailOnWrite {
        fn failing_at(call: usize) -> Self {
            Self {
                calls: Cell::new(0),
                fail_at: call,
            }
        }

        fn calls(&self) -> usize {
            self.calls.get()
        }
    }

    impl Write for FailOnWrite {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.calls.set(self.calls.get() + 1);
            if self.calls.get() == self.fail_at {
                return Err(io::Error::other("boom"));
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn natural_completion_opens_and_closes_period_and_notifies() {
        let config = config(Duration::ZERO, true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::working();
        let notifier = FakeNotifier::default();
        let (_tx, rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = run(
            &config,
            &clock,
            Some(&mut ledger),
            &notifier,
            &rx,
            &mut state,
            &mut diagnostics,
        );

        assert_eq!(
            outcome,
            Outcome::Completed {
                displayed: true,
                closed: true
            }
        );
        assert_eq!(*lock(&ledger.opened_with), vec!["work".to_owned()]);
        assert_eq!(*lock(&ledger.closes), 1);
        assert_eq!(
            notifier.calls(),
            vec![(TITLE.to_owned(), MESSAGE.to_owned())]
        );
        assert_eq!(
            text(&state),
            "\r12:34 => 00:00 => 12:34\r12:34 => 00:00 => 12:36\n"
        );
        assert_eq!(text(&diagnostics), "");
        assert!(
            lock(&clock.local_minutes).is_empty(),
            "the clock is read exactly for the start and the finish"
        );
    }

    #[test]
    fn natural_completion_without_account_needs_no_ledger() {
        let config = config(Duration::ZERO, false);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let notifier = FakeNotifier::default();
        let (_tx, rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = run(
            &config,
            &clock,
            None,
            &notifier,
            &rx,
            &mut state,
            &mut diagnostics,
        );

        assert_eq!(
            outcome,
            Outcome::Completed {
                displayed: true,
                closed: true
            }
        );
        assert_eq!(
            notifier.calls(),
            vec![(TITLE.to_owned(), MESSAGE.to_owned())]
        );
        assert_eq!(
            text(&state),
            "\r12:34 => 00:00 => 12:34\r12:34 => 00:00 => 12:36\n"
        );
        assert_eq!(text(&diagnostics), "");
    }

    #[test]
    fn natural_completion_notifies_even_when_close_fails() {
        let config = config(Duration::ZERO, true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::failing_to_close("cannot write /tmp/time.ledger: disk full");
        let notifier = FakeNotifier::default();
        let (_tx, rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = run(
            &config,
            &clock,
            Some(&mut ledger),
            &notifier,
            &rx,
            &mut state,
            &mut diagnostics,
        );

        assert_eq!(
            outcome,
            Outcome::Completed {
                displayed: true,
                closed: false
            }
        );
        assert_eq!(
            notifier.calls(),
            vec![(TITLE.to_owned(), MESSAGE.to_owned())]
        );
        assert_eq!(
            text(&diagnostics),
            "cannot write /tmp/time.ledger: disk full\n"
        );
    }

    #[test]
    fn natural_completion_reports_display_error_only_after_notifying() {
        let config = config(Duration::ZERO, true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::working();
        let notifier = BellNotifier;
        let (_tx, rx) = mpsc::channel();
        let mut state = FailOnWrite::failing_at(2);
        let mut diagnostics = Vec::new();

        let outcome = run(
            &config,
            &clock,
            Some(&mut ledger),
            &notifier,
            &rx,
            &mut state,
            &mut diagnostics,
        );

        assert_eq!(
            outcome,
            Outcome::Completed {
                displayed: false,
                closed: true
            }
        );
        assert_eq!(state.calls(), 2);
        assert_eq!(
            text(&diagnostics),
            "\x07Timer finished\ncannot display timer: boom\n"
        );
    }

    #[test]
    fn open_failure_reports_and_skips_close_and_display() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![]);
        let mut ledger = FakeLedger::working();
        ledger.open_result = Err(LedgerError::new("cannot write /tmp/time.ledger: disk full"));
        let notifier = FakeNotifier::default();
        let (_tx, rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = run(
            &config,
            &clock,
            Some(&mut ledger),
            &notifier,
            &rx,
            &mut state,
            &mut diagnostics,
        );

        assert_eq!(outcome, Outcome::Failed);
        assert_eq!(*lock(&ledger.opened_with), vec!["work".to_owned()]);
        assert_eq!(*lock(&ledger.closes), 0);
        assert_eq!(
            text(&diagnostics),
            "cannot write /tmp/time.ledger: disk full\n"
        );
        assert_eq!(text(&state), "");
        assert!(notifier.calls().is_empty());
        assert!(lock(&clock.local_minutes).is_empty());
    }

    #[test]
    fn initial_clock_failure_reports_and_closes_period() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![Err(ClockError::new("cannot run date: stubbed"))]);
        let mut ledger = FakeLedger::working();
        let notifier = FakeNotifier::default();
        let (_tx, rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = run(
            &config,
            &clock,
            Some(&mut ledger),
            &notifier,
            &rx,
            &mut state,
            &mut diagnostics,
        );

        assert_eq!(outcome, Outcome::Failed);
        assert_eq!(*lock(&ledger.opened_with), vec!["work".to_owned()]);
        assert_eq!(*lock(&ledger.closes), 1);
        assert_eq!(
            text(&diagnostics),
            "cannot display timer: cannot run date: stubbed\n"
        );
        assert_eq!(text(&state), "");
        assert!(notifier.calls().is_empty());
    }

    #[test]
    fn initial_state_write_failure_reports_and_closes_period() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![Ok(time(12, 34))]);
        let mut ledger = FakeLedger::working();
        let notifier = FakeNotifier::default();
        let (_tx, rx) = mpsc::channel();
        let mut state = FailOnWrite::failing_at(1);
        let mut diagnostics = Vec::new();

        let outcome = run(
            &config,
            &clock,
            Some(&mut ledger),
            &notifier,
            &rx,
            &mut state,
            &mut diagnostics,
        );

        assert_eq!(outcome, Outcome::Failed);
        assert_eq!(*lock(&ledger.closes), 1);
        assert_eq!(text(&diagnostics), "cannot display timer: boom\n");
        assert!(notifier.calls().is_empty());
    }

    #[test]
    fn interrupt_closes_period_and_skips_notification() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::working();
        let notifier = FakeNotifier::default();
        let (tx, rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = thread::scope(|scope| {
            scope.spawn(move || tx.send(Event::Interrupt).expect("send interrupt"));
            run(
                &config,
                &clock,
                Some(&mut ledger),
                &notifier,
                &rx,
                &mut state,
                &mut diagnostics,
            )
        });

        assert_eq!(outcome, Outcome::Interrupted { closed: true });
        assert_eq!(*lock(&ledger.closes), 1);
        assert!(notifier.calls().is_empty());
        assert_eq!(
            text(&state),
            "\r12:34 => 00:05 => 12:39\r12:34 => 00:05 => 12:36\n"
        );
        assert_eq!(text(&diagnostics), "");
    }

    #[test]
    fn interrupt_with_close_failure_keeps_interrupt_outcome() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::failing_to_close("cannot write /tmp/time.ledger: disk full");
        let notifier = FakeNotifier::default();
        let (tx, rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = thread::scope(|scope| {
            scope.spawn(move || tx.send(Event::Interrupt).expect("send interrupt"));
            run(
                &config,
                &clock,
                Some(&mut ledger),
                &notifier,
                &rx,
                &mut state,
                &mut diagnostics,
            )
        });

        assert_eq!(outcome, Outcome::Interrupted { closed: false });
        assert_eq!(
            text(&diagnostics),
            "cannot write /tmp/time.ledger: disk full\n"
        );
        assert!(notifier.calls().is_empty());
    }

    #[test]
    fn interrupt_reports_display_error_without_changing_outcome() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::working();
        let notifier = FakeNotifier::default();
        let (tx, rx) = mpsc::channel();
        let mut state = FailOnWrite::failing_at(2);
        let mut diagnostics = Vec::new();

        let outcome = thread::scope(|scope| {
            scope.spawn(move || tx.send(Event::Interrupt).expect("send interrupt"));
            run(
                &config,
                &clock,
                Some(&mut ledger),
                &notifier,
                &rx,
                &mut state,
                &mut diagnostics,
            )
        });

        assert_eq!(outcome, Outcome::Interrupted { closed: true });
        assert_eq!(state.calls(), 2);
        assert_eq!(text(&diagnostics), "cannot display timer: boom\n");
        assert!(notifier.calls().is_empty());
    }

    #[test]
    fn suspend_closes_period_before_releasing() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::working();
        let notifier = FakeNotifier::default();
        let (tx, rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = thread::scope(|scope| {
            scope.spawn(move || tx.send(Event::Suspend(release_tx)).expect("send suspend"));
            run(
                &config,
                &clock,
                Some(&mut ledger),
                &notifier,
                &rx,
                &mut state,
                &mut diagnostics,
            )
        });

        assert_eq!(outcome, Outcome::Suspended { closed: true });
        assert_eq!(*lock(&ledger.closes), 1);
        assert!(notifier.calls().is_empty());
        release_rx.recv().expect("release acknowledged");
        ledger.record("release");
        assert_eq!(
            *lock(&ledger.events),
            vec!["open".to_owned(), "close".to_owned(), "release".to_owned()]
        );
        assert_eq!(
            text(&state),
            "\r12:34 => 00:05 => 12:39\r12:34 => 00:05 => 12:36\n"
        );
        assert_eq!(
            text(&diagnostics),
            "System suspend detected; timer stopped\n"
        );
    }

    #[test]
    fn suspend_releases_after_failed_close_attempt() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::failing_to_close("cannot write /tmp/time.ledger: disk full");
        let notifier = FakeNotifier::default();
        let (tx, rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = thread::scope(|scope| {
            scope.spawn(move || tx.send(Event::Suspend(release_tx)).expect("send suspend"));
            run(
                &config,
                &clock,
                Some(&mut ledger),
                &notifier,
                &rx,
                &mut state,
                &mut diagnostics,
            )
        });

        assert_eq!(outcome, Outcome::Suspended { closed: false });
        release_rx.recv().expect("release acknowledged");
        ledger.record("release");
        assert_eq!(
            *lock(&ledger.events),
            vec!["open".to_owned(), "close".to_owned(), "release".to_owned()]
        );
        assert_eq!(
            text(&diagnostics),
            "cannot write /tmp/time.ledger: disk full\nSystem suspend detected; timer stopped\n"
        );
    }

    #[test]
    fn monitor_failure_reports_and_closes_period() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::working();
        let notifier = FakeNotifier::default();
        let (tx, rx) = mpsc::channel();
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = thread::scope(|scope| {
            scope.spawn(move || {
                tx.send(Event::MonitorFailed("dbus exploded".to_owned()))
                    .expect("send monitor failure");
            });
            run(
                &config,
                &clock,
                Some(&mut ledger),
                &notifier,
                &rx,
                &mut state,
                &mut diagnostics,
            )
        });

        assert_eq!(outcome, Outcome::Failed);
        assert_eq!(*lock(&ledger.closes), 1);
        assert_eq!(
            text(&diagnostics),
            "system sleep monitor failed: dbus exploded\n"
        );
        assert_eq!(
            text(&state),
            "\r12:34 => 00:05 => 12:39\r12:34 => 00:05 => 12:36\n"
        );
        assert!(notifier.calls().is_empty());
    }

    #[test]
    fn channel_disconnect_reports_and_closes_period() {
        let config = config(Duration::from_secs(300), true);
        let clock = FakeClock::local(vec![Ok(time(12, 34)), Ok(time(12, 36))]);
        let mut ledger = FakeLedger::working();
        let notifier = FakeNotifier::default();
        let (tx, rx) = mpsc::channel();
        drop(tx);
        let mut state = Vec::new();
        let mut diagnostics = Vec::new();

        let outcome = run(
            &config,
            &clock,
            Some(&mut ledger),
            &notifier,
            &rx,
            &mut state,
            &mut diagnostics,
        );

        assert_eq!(outcome, Outcome::Failed);
        assert_eq!(*lock(&ledger.closes), 1);
        assert_eq!(text(&diagnostics), "timer event channel disconnected\n");
        assert_eq!(
            text(&state),
            "\r12:34 => 00:05 => 12:39\r12:34 => 00:05 => 12:36\n"
        );
        assert!(notifier.calls().is_empty());
    }
}
