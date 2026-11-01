use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::PathBuf,
    process::{self, Command},
    sync::mpsc::{self, Receiver, RecvTimeoutError, Sender},
    thread,
    time::{Duration, Instant},
};
use zbus::{
    blocking::{Connection, Proxy},
    zvariant::OwnedFd,
};

const DISPLAY_INTERVAL: Duration = Duration::from_secs(60);
const SUSPEND_RELEASE_TIMEOUT: Duration = Duration::from_secs(2);

struct Config {
    account: Option<String>,
    duration: Duration,
}

enum Event {
    Interrupt,
    Suspend(Sender<()>),
    MonitorFailed(String),
}

struct Ledger {
    file: File,
    path: PathBuf,
    account: String,
    needs_newline: bool,
    active: bool,
}

impl Ledger {
    fn open(account: &str) -> Result<Self, String> {
        let path = env::var_os("LEDGER_FILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| "LEDGER_FILE is required when an account is provided".to_owned())?;
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
        let mut contents = String::new();
        file.try_clone()
            .and_then(|mut reader| reader.read_to_string(&mut contents))
            .map_err(|error| format!("cannot read {}: {error}", path.display()))?;

        if contents
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .is_some_and(|line| line.starts_with("i "))
        {
            return Err(format!(
                "{} ends with an open period; close it before starting a new one",
                path.display()
            ));
        }

        Ok(Self {
            file,
            path,
            account: account.to_owned(),
            needs_newline: !contents.is_empty() && !contents.ends_with('\n'),
            active: false,
        })
    }

    fn start(&mut self) -> Result<(), String> {
        let entry = format!("i {} {}\n", local_timestamp()?, self.account);
        self.append(&entry)?;
        self.active = true;
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        if !self.active {
            return Ok(());
        }
        let entry = format!("o {}\n", local_timestamp()?);
        self.append(&entry)?;
        self.active = false;
        Ok(())
    }

    fn append(&mut self, entry: &str) -> Result<(), String> {
        let prefix = if self.needs_newline { "\n" } else { "" };
        let entry = format!("{prefix}{entry}");
        self.file
            .write_all(entry.as_bytes())
            .and_then(|()| self.file.flush())
            .map_err(|error| format!("cannot write {}: {error}", self.path.display()))?;
        self.needs_newline = false;
        Ok(())
    }
}

fn main() {
    process::exit(run());
}

fn run() -> i32 {
    let config = match Config::parse() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}\nUsage: t <minutes> | t <account> <minutes>");
            return 2;
        }
    };

    let mut ledger = match config.account.as_deref().map(Ledger::open).transpose() {
        Ok(ledger) => ledger,
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };

    let (event_tx, event_rx) = mpsc::channel();
    let interrupt_tx = event_tx.clone();
    if let Err(error) = ctrlc::set_handler(move || {
        let _ = interrupt_tx.send(Event::Interrupt);
    }) {
        eprintln!("cannot install Ctrl+C handler: {error}");
        return 1;
    }

    if let Err(error) = spawn_sleep_monitor(event_tx) {
        eprintln!("cannot monitor system sleep: {error}");
        return 1;
    }

    if let Some(ledger) = ledger.as_mut()
        && let Err(error) = ledger.start()
    {
        eprintln!("{error}");
        return 1;
    }

    run_timer(config.duration, ledger, event_rx)
}

impl Config {
    fn parse() -> Result<Self, String> {
        let arguments: Vec<String> = env::args().skip(1).collect();
        let (account, minutes) = match arguments.as_slice() {
            [minutes] => (None, minutes),
            [account, minutes] if !account.is_empty() => {
                if account.contains(['\n', '\r']) {
                    return Err("account cannot contain a newline".to_owned());
                }
                (Some(account.clone()), minutes)
            }
            [_account, _minutes] => return Err("account cannot be empty".to_owned()),
            _ => return Err("invalid arguments".to_owned()),
        };
        let minutes = minutes
            .parse::<u64>()
            .ok()
            .filter(|minutes| *minutes >= 1)
            .ok_or_else(|| "minutes must be a positive integer".to_owned())?;
        let seconds = minutes
            .checked_mul(60)
            .ok_or_else(|| "duration is too large".to_owned())?;
        let duration = Duration::from_secs(seconds);
        if Instant::now().checked_add(duration).is_none() {
            return Err("duration is too large".to_owned());
        }
        Ok(Self { account, duration })
    }
}

fn run_timer(duration: Duration, mut ledger: Option<Ledger>, events: Receiver<Event>) -> i32 {
    let start = Instant::now();
    let deadline = start + duration;
    let mut next_display = start + DISPLAY_INTERVAL;

    if let Err(error) = display_remaining(duration) {
        eprintln!("cannot display timer: {error}");
        return close_after_error(ledger);
    }

    loop {
        let now = Instant::now();
        if now >= deadline {
            if let Err(error) = display_remaining(Duration::ZERO) {
                eprintln!("cannot display timer: {error}");
                return close_after_error(ledger);
            }
            println!();
            let close_result = close_ledger(&mut ledger);
            notify();
            return report_close_result(close_result);
        }

        let wake_at = next_display.min(deadline);
        match events.recv_timeout(wake_at.saturating_duration_since(now)) {
            Ok(Event::Interrupt) => {
                println!();
                return match close_ledger(&mut ledger) {
                    Ok(()) => 130,
                    Err(()) => 1,
                };
            }
            Ok(Event::Suspend(release)) => {
                println!();
                let close_result = close_ledger(&mut ledger);
                let _ = release.send(());
                if close_result.is_err() {
                    return 1;
                }
                eprintln!("System suspend detected; timer stopped");
                return 0;
            }
            Ok(Event::MonitorFailed(error)) => {
                println!();
                eprintln!("system sleep monitor failed: {error}");
                return close_after_error(ledger);
            }
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                if now >= next_display && now < deadline {
                    if let Err(error) = display_remaining(deadline.saturating_duration_since(now)) {
                        eprintln!("cannot display timer: {error}");
                        return close_after_error(ledger);
                    }
                    while next_display <= now {
                        next_display += DISPLAY_INTERVAL;
                    }
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                eprintln!("timer event channel disconnected");
                return close_after_error(ledger);
            }
        }
    }
}

fn close_ledger(ledger: &mut Option<Ledger>) -> Result<(), ()> {
    match ledger.as_mut().map(Ledger::finish).transpose() {
        Ok(_) => Ok(()),
        Err(error) => {
            eprintln!("{error}");
            Err(())
        }
    }
}

fn close_after_error(mut ledger: Option<Ledger>) -> i32 {
    let _ = close_ledger(&mut ledger);
    1
}

fn report_close_result(result: Result<(), ()>) -> i32 {
    if result.is_ok() { 0 } else { 1 }
}

fn display_remaining(remaining: Duration) -> io::Result<()> {
    let seconds = remaining.as_secs() + u64::from(remaining.subsec_nanos() != 0);
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let seconds = seconds % 60;
    print!("\r{hours:02}:{minutes:02}:{seconds:02}");
    io::stdout().flush()
}

fn local_timestamp() -> Result<String, String> {
    let output = Command::new("date")
        .arg("+%Y/%m/%d %H:%M:%S")
        .output()
        .map_err(|error| format!("cannot run date: {error}"))?;
    if !output.status.success() {
        return Err(format!("date exited with {}", output.status));
    }
    String::from_utf8(output.stdout)
        .map(|timestamp| timestamp.trim().to_owned())
        .map_err(|error| format!("date returned invalid UTF-8: {error}"))
}

fn notify() {
    let sent = Command::new("notify-send")
        .args(["Timer finished", "Time is up"])
        .status()
        .is_ok_and(|status| status.success());
    if !sent {
        eprintln!("\x07Timer finished");
    }
}

fn spawn_sleep_monitor(events: Sender<Event>) -> Result<(), String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::Builder::new()
        .name("sleep-monitor".to_owned())
        .spawn(move || {
            if let Err(error) = run_sleep_monitor(&events, &ready_tx) {
                let _ = ready_tx.send(Err(error.clone()));
                let _ = events.send(Event::MonitorFailed(error));
            }
        })
        .map_err(|error| error.to_string())?;

    match ready_rx.recv() {
        Ok(Ok(())) => Ok(()),
        Ok(Err(error)) => Err(error),
        Err(_) => Err("sleep monitor stopped during startup".to_owned()),
    }
}

fn run_sleep_monitor(
    events: &Sender<Event>,
    ready: &Sender<Result<(), String>>,
) -> Result<(), String> {
    let connection = Connection::system()
        .map_err(|error| format!("cannot connect to the system D-Bus: {error}"))?;
    let proxy = Proxy::new(
        &connection,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .map_err(|error| format!("cannot create logind proxy: {error}"))?;
    let mut signals = proxy
        .receive_signal("PrepareForSleep")
        .map_err(|error| format!("cannot subscribe to PrepareForSleep: {error}"))?;
    let _inhibitor: OwnedFd = proxy
        .call(
            "Inhibit",
            &("sleep", "t", "Close the active time period", "delay"),
        )
        .map_err(|error| format!("cannot acquire sleep delay inhibitor: {error}"))?;

    ready
        .send(Ok(()))
        .map_err(|_| "timer stopped during sleep monitor startup".to_owned())?;

    for signal in &mut signals {
        let preparing = signal
            .body()
            .deserialize::<bool>()
            .map_err(|error| format!("invalid PrepareForSleep signal: {error}"))?;
        if preparing {
            let (release_tx, release_rx) = mpsc::channel();
            if events.send(Event::Suspend(release_tx)).is_err() {
                return Ok(());
            }
            let _ = release_rx.recv_timeout(SUSPEND_RELEASE_TIMEOUT);
            return Ok(());
        }
    }

    Err("PrepareForSleep signal stream ended".to_owned())
}
