#![forbid(unsafe_code)]

use std::{io, process, sync::mpsc};

use t::adapters::{
    args, date_clock::DateClock, file_ledger::FileLedger, interrupt, notifier, sleep_monitor,
};
use t::application::{self, outcome::Outcome, ports::LedgerWriter};

fn main() {
    process::exit(run());
}

fn run() -> i32 {
    let config = match args::parse() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}\nUsage: t <minutes> | t <account> <minutes>");
            return 2;
        }
    };

    let clock = DateClock;
    let mut ledger = if config.account.is_some() {
        match FileLedger::open(&clock) {
            Ok(ledger) => Some(ledger),
            Err(error) => {
                eprintln!("{error}");
                return 1;
            }
        }
    } else {
        None
    };

    let (event_tx, event_rx) = mpsc::channel();
    if let Err(error) = interrupt::install(event_tx.clone()) {
        eprintln!("{error}");
        return 1;
    }

    if let Err(error) = sleep_monitor::spawn(event_tx) {
        eprintln!("cannot monitor system sleep: {error}");
        return 1;
    }

    let notifier = notifier::NotifySend;
    let mut state = io::stdout();
    let mut diagnostics = io::stderr();
    let outcome = application::timer::run(
        &config,
        &clock,
        ledger
            .as_mut()
            .map(|ledger| ledger as &mut dyn LedgerWriter),
        &notifier,
        &event_rx,
        &mut state,
        &mut diagnostics,
    );
    exit_code(outcome)
}

/// Переводит исход прогона в код выхода процесса. Это единственное место,
/// где выбирается код.
fn exit_code(outcome: Outcome) -> i32 {
    match outcome {
        Outcome::Completed { displayed, closed } => {
            if displayed && closed {
                0
            } else {
                1
            }
        }
        Outcome::Interrupted { closed } => {
            if closed {
                130
            } else {
                1
            }
        }
        Outcome::Suspended { closed } => {
            if closed {
                0
            } else {
                1
            }
        }
        Outcome::Failed => 1,
    }
}
