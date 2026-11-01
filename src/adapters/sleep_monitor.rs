//! Наблюдение за сном системы: системная шина D-Bus, подписка на
//! `PrepareForSleep` и sleep delay inhibitor.

use std::sync::mpsc::{self, Sender};
use std::thread;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedFd;

use crate::application::event::Event;

/// Запускает поток наблюдения за сном и дожидается рукопожатия готовности:
/// inhibitor захвачен до старта Таймера, чтобы запись Учитываемого
/// периода успевала закрыться перед сном.
pub fn spawn(events: Sender<Event>) -> Result<(), String> {
    let (ready_tx, ready_rx) = mpsc::channel();
    thread::Builder::new()
        .name("sleep-monitor".to_owned())
        .spawn(move || {
            if let Err(error) = run(&events, &ready_tx) {
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

fn run(events: &Sender<Event>, ready: &Sender<Result<(), String>>) -> Result<(), String> {
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
            // Без собственного таймаута: inhibitor освобождается только
            // после того, как сценарий подтвердит освобождение.
            let _ = release_rx.recv();
            return Ok(());
        }
    }

    Err("PrepareForSleep signal stream ended".to_owned())
}
