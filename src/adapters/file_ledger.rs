//! Хранилище Ledger на основе файла.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::application::ports::{Clock, LedgerError, LedgerWriter};
use crate::domain::account::Account;
use crate::domain::ledger::{LedgerEntry, previous_period_is_open};

/// Файл Ledger: записи начала и конца Учитываемого периода. Обнаружение
/// незакрытого предыдущего периода делегирует доменному правилу.
pub struct FileLedger<'a> {
    file: File,
    path: PathBuf,
    clock: &'a dyn Clock,
    needs_newline: bool,
    active: bool,
}

impl<'a> FileLedger<'a> {
    /// Открывает файл Ledger: проверяет `LEDGER_FILE`, открывает файл и
    /// отклоняет незакрытый предыдущий период.
    pub fn open(clock: &'a dyn Clock) -> Result<Self, LedgerError> {
        let path = std::env::var_os("LEDGER_FILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| {
                LedgerError::new("LEDGER_FILE is required when an account is provided")
            })?;
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|error| {
                LedgerError::new(format!("cannot open {}: {error}", path.display()))
            })?;
        let mut contents = String::new();
        file.try_clone()
            .and_then(|mut reader| reader.read_to_string(&mut contents))
            .map_err(|error| {
                LedgerError::new(format!("cannot read {}: {error}", path.display()))
            })?;

        if previous_period_is_open(&contents) {
            return Err(LedgerError::new(format!(
                "{} ends with an open period; close it before starting a new one",
                path.display()
            )));
        }

        Ok(Self {
            file,
            path,
            clock,
            needs_newline: !contents.is_empty() && !contents.ends_with('\n'),
            active: false,
        })
    }

    fn append(&mut self, entry: &str) -> Result<(), LedgerError> {
        let prefix = if self.needs_newline { "\n" } else { "" };
        let entry = format!("{prefix}{entry}");
        self.file
            .write_all(entry.as_bytes())
            .and_then(|()| self.file.flush())
            .map_err(|error| {
                LedgerError::new(format!("cannot write {}: {error}", self.path.display()))
            })?;
        self.needs_newline = false;
        Ok(())
    }
}

impl LedgerWriter for FileLedger<'_> {
    fn open_period(&mut self, account: &Account) -> Result<(), LedgerError> {
        let entry = LedgerEntry::start(self.clock.ledger_timestamp()?, account.clone());
        self.append(&entry.render())?;
        self.active = true;
        Ok(())
    }

    fn close_period(&mut self) -> Result<(), LedgerError> {
        if !self.active {
            return Ok(());
        }
        let entry = LedgerEntry::finish(self.clock.ledger_timestamp()?);
        self.append(&entry.render())?;
        self.active = false;
        Ok(())
    }
}
