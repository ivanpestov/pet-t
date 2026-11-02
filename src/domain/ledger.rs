//! Ledger: записи начала и конца Учитываемого периода и правило незакрытого
//! предыдущего периода.

use crate::domain::{account::Account, time::Timestamp};

/// Запись Ledger о границе Учитываемого периода.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerEntry {
    Start {
        timestamp: Timestamp,
        account: Account,
    },
    Finish {
        timestamp: Timestamp,
    },
}

impl LedgerEntry {
    pub fn start(timestamp: Timestamp, account: Account) -> Self {
        Self::Start { timestamp, account }
    }

    pub fn finish(timestamp: Timestamp) -> Self {
        Self::Finish { timestamp }
    }

    pub fn render(&self) -> String {
        match self {
            Self::Start { timestamp, account } => {
                format!("i {} {}\n", timestamp.as_str(), account.as_str())
            }
            Self::Finish { timestamp } => format!("o {}\n", timestamp.as_str()),
        }
    }
}

/// Возвращает true, если последняя непустая строка содержимого Ledger
/// открывает Учитываемый период и не закрывает его.
pub fn previous_period_is_open(contents: &str) -> bool {
    contents
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| line.starts_with("i "))
}

#[cfg(test)]
mod tests {
    use super::{LedgerEntry, previous_period_is_open};
    use crate::domain::{account::Account, time::Timestamp};

    #[test]
    fn renders_a_start_entry_with_timestamp_and_account() {
        let entry = LedgerEntry::start(timestamp(), account("work"));
        assert_eq!(entry.render(), "i 2026/11/01 12:34:56 work\n");
    }

    #[test]
    fn renders_a_start_entry_with_a_multi_segment_account() {
        let entry = LedgerEntry::start(timestamp(), account("work:project"));
        assert_eq!(entry.render(), "i 2026/11/01 12:34:56 work:project\n");
    }

    #[test]
    fn renders_a_finish_entry_with_timestamp_only() {
        let entry = LedgerEntry::finish(timestamp());
        assert_eq!(entry.render(), "o 2026/11/01 12:34:56\n");
    }

    #[test]
    fn an_empty_ledger_has_no_open_period() {
        assert!(!previous_period_is_open(""));
        assert!(!previous_period_is_open("\n"));
        assert!(!previous_period_is_open("\n\n  \n"));
    }

    #[test]
    fn a_ledger_ending_with_a_start_entry_has_an_open_period() {
        assert!(previous_period_is_open("i 2026/11/01 12:34:56 work\n"));
        assert!(previous_period_is_open("i 2026/11/01 12:34:56 work"));
    }

    #[test]
    fn a_ledger_ending_with_a_finish_entry_has_no_open_period() {
        assert!(!previous_period_is_open(
            "i 2026/11/01 12:34:56 work\no 2026/11/01 12:36:00\n"
        ));
        assert!(!previous_period_is_open("o 2026/11/01 12:36:00\n"));
    }

    #[test]
    fn trailing_empty_lines_do_not_hide_the_last_entry() {
        assert!(previous_period_is_open("i 2026/11/01 12:34:56 work\n\n\n"));
        assert!(!previous_period_is_open(
            "i 2026/11/01 12:34:56 work\no 2026/11/01 12:36:00\n \n\t\n"
        ));
    }

    #[test]
    fn the_open_marker_requires_the_entry_prefix() {
        assert!(!previous_period_is_open("i\n"));
        assert!(!previous_period_is_open(
            "i 2026/11/01 12:34:56 work\nindigo 2026/11/01 12:36:00\n"
        ));
    }

    fn timestamp() -> Timestamp {
        Timestamp::new("2026/11/01 12:34:56")
    }

    fn account(raw: &str) -> Account {
        Account::new(raw).expect("valid account")
    }
}
