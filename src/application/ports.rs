//! Порты прикладного слоя к внешнему миру.

use std::fmt;
use std::io::Write;

use crate::domain::account::Account;
use crate::domain::time::{MinuteOfDay, Timestamp};

/// Часы: локальное время суток и метка Ledger. Реализация живёт в адаптере,
/// прикладной слой зависит только от этого трейта.
pub trait Clock {
    fn local_minutes(&self) -> Result<MinuteOfDay, ClockError>;
    fn ledger_timestamp(&self) -> Result<Timestamp, ClockError>;
}

/// Ошибка чтения времени. Сообщение сформировано реализацией порта и
/// готово для вывода в диагностику.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockError(String);

impl ClockError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for ClockError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ClockError {}

/// Хранилище Ledger: открытие и закрытие Учитываемого периода. Порт
/// называется не `Ledger`, чтобы не перегружать доменный термин.
pub trait LedgerWriter {
    fn open_period(&mut self, account: &Account) -> Result<(), LedgerError>;
    fn close_period(&mut self) -> Result<(), LedgerError>;
}

/// Ошибка работы с хранилищем Ledger. Сообщение сформировано реализацией
/// порта и готово для вывода в диагностику.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerError(String);

impl LedgerError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for LedgerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LedgerError {}

impl From<ClockError> for LedgerError {
    fn from(error: ClockError) -> Self {
        Self(error.to_string())
    }
}

/// Уведомления. Отправка не влияет на код выхода, поэтому ошибок порт не
/// возвращает; fallback выполняет реализация через переданный поток
/// диагностики.
pub trait Notifier {
    fn notify(&self, title: &str, message: &str, diagnostics: &mut dyn Write);
}
