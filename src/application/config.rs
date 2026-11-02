//! Конфигурация прогона Таймера.

use std::time::Duration;

use crate::domain::account::Account;

/// Конфигурация Таймера: Account при его наличии и длительность. Account
/// валидируется конструктором на границе CLI, поэтому здесь он уже
/// проверен.
pub struct Config {
    pub account: Option<Account>,
    pub duration: Duration,
}
