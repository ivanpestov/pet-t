//! Адаптеры к внешнему миру: разбор аргументов, файл Ledger, системные
//! команды, уведомления, Ctrl+C и наблюдение за сном.

pub mod args;
pub mod completion;
pub mod date_clock;
pub mod file_ledger;
pub mod interrupt;
pub mod notifier;
pub mod sleep_monitor;
