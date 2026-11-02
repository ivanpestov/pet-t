//! Разбор аргументов командной строки.

use std::time::{Duration, Instant};

use crate::application::config::Config;
use crate::domain::account::Account;

/// Ошибка разбора аргументов. Тексты совпадают с прежними сообщениями
/// бинарника; справка по использованию и код 2 остаются на границе CLI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgsError {
    InvalidAccount,
    InvalidArguments,
    InvalidMinutes,
    DurationTooLarge,
}

impl std::fmt::Display for ArgsError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::InvalidAccount => "invalid account",
            Self::InvalidArguments => "invalid arguments",
            Self::InvalidMinutes => "minutes must be a positive integer",
            Self::DurationTooLarge => "duration is too large",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for ArgsError {}

/// Запрос скрытого режима автодополнения: текущее и предыдущее слово
/// командной строки по протоколу `t --complete <текущее> <предыдущее>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionRequest {
    pub current: String,
    pub previous: String,
}

/// Обнаруживает скрытый режим автодополнения до обычного разбора
/// аргументов; любой другой набор аргументов передаётся обычному разбору.
pub fn completion_request() -> Option<CompletionRequest> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [flag, current, previous] if flag == "--complete" => Some(CompletionRequest {
            current: current.clone(),
            previous: previous.clone(),
        }),
        _ => None,
    }
}

/// Разбирает аргументы процесса в Config. Account валидируется
/// конструктором на этой границе и дальше передаётся проверенным.
pub fn parse() -> Result<Config, ArgsError> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let (account, minutes) = match arguments.as_slice() {
        [minutes] => (None, minutes),
        [account, minutes] => {
            let account = Account::new(account).map_err(|_| ArgsError::InvalidAccount)?;
            (Some(account), minutes)
        }
        _ => return Err(ArgsError::InvalidArguments),
    };
    let minutes = minutes
        .parse::<u64>()
        .ok()
        .filter(|minutes| *minutes >= 1)
        .ok_or(ArgsError::InvalidMinutes)?;
    let seconds = minutes.checked_mul(60).ok_or(ArgsError::DurationTooLarge)?;
    let duration = Duration::from_secs(seconds);
    if Instant::now().checked_add(duration).is_none() {
        return Err(ArgsError::DurationTooLarge);
    }
    Ok(Config { account, duration })
}
