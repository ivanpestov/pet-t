//! Account: иерархическая категория учёта, валидируемая один раз при
//! создании.

use unicode_ident::{is_xid_continue, is_xid_start};

/// Провалидированный Account. Значение прошло проверку сегментов при
/// создании, поэтому дальше по вызовам его можно передавать без повторной
/// валидации.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account(String);

/// Ошибка создания Account из невалидной строки.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidAccount;

impl Account {
    pub fn new(raw: &str) -> Result<Self, InvalidAccount> {
        if is_valid_account(raw) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(InvalidAccount)
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InvalidAccount {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("invalid account")
    }
}

impl std::error::Error for InvalidAccount {}

fn is_valid_account(account: &str) -> bool {
    account.split(':').all(|segment| {
        if segment == "_" || is_rust_keyword(segment) {
            return false;
        }
        let mut chars = segment.chars();
        chars
            .next()
            .is_some_and(|first| first == '_' || is_xid_start(first))
            && chars.all(is_xid_continue)
    })
}

fn is_rust_keyword(segment: &str) -> bool {
    matches!(
        segment,
        "as" | "async"
            | "await"
            | "break"
            | "const"
            | "continue"
            | "crate"
            | "dyn"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "gen"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "try"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
    )
}

#[cfg(test)]
mod tests {
    use super::{Account, InvalidAccount};

    #[test]
    fn accepts_a_valid_account_verbatim() {
        let account = Account::new("work:project").expect("valid account");
        assert_eq!(account.as_str(), "work:project");
    }

    #[test]
    fn accepts_single_and_multi_segment_accounts() {
        for raw in ["work", "work:project", "a:b:c"] {
            assert!(Account::new(raw).is_ok(), "account: {raw:?}");
        }
    }

    #[test]
    fn accepts_leading_and_inner_underscores() {
        for raw in ["_work", "__:selfish", "_work:project_2", "work_"] {
            assert!(Account::new(raw).is_ok(), "account: {raw:?}");
        }
    }

    #[test]
    fn accepts_digits_outside_the_first_position() {
        for raw in ["work2", "async_:gen2", "δ2", "проект2"] {
            assert!(Account::new(raw).is_ok(), "account: {raw:?}");
        }
    }

    #[test]
    fn accepts_unicode_segments() {
        for raw in ["работа:проект2", "e\u{301}:cafe\u{301}", "δ2:任务_3"] {
            assert!(Account::new(raw).is_ok(), "account: {raw:?}");
        }
    }

    #[test]
    fn accepts_accounts_differing_from_keywords_by_case() {
        for raw in ["Work:work", "SELF", "Fn", "ASYNC"] {
            assert!(Account::new(raw).is_ok(), "account: {raw:?}");
        }
    }

    #[test]
    fn rejects_rust_keywords_in_any_segment() {
        for raw in [
            "fn",
            "async",
            "type",
            "unsafe",
            "gen",
            "self",
            "Self",
            "work:async",
        ] {
            assert_eq!(Account::new(raw), Err(InvalidAccount), "account: {raw:?}");
        }
    }

    #[test]
    fn rejects_a_single_underscore() {
        for raw in ["_", "work:_", "_:_"] {
            assert!(Account::new(raw).is_err(), "account: {raw:?}");
        }
    }

    #[test]
    fn rejects_raw_identifier_syntax() {
        for raw in ["r#type", "work:r#project"] {
            assert!(Account::new(raw).is_err(), "account: {raw:?}");
        }
    }

    #[test]
    fn rejects_empty_segments() {
        for raw in ["", ":work", "work:", "work::project", "::"] {
            assert!(Account::new(raw).is_err(), "account: {raw:?}");
        }
    }

    #[test]
    fn rejects_a_leading_digit() {
        for raw in ["1work", "work:2project", "9"] {
            assert!(Account::new(raw).is_err(), "account: {raw:?}");
        }
    }

    #[test]
    fn rejects_spaces_and_newlines() {
        for raw in [
            "work project",
            "work:project alpha",
            "work\nproject",
            "work\rproject",
            "work\tproject",
        ] {
            assert!(Account::new(raw).is_err(), "account: {raw:?}");
        }
    }

    #[test]
    fn rejects_punctuation_outside_separators() {
        for raw in [
            "work-project",
            "work:project!",
            "work/project",
            "work.project",
        ] {
            assert!(Account::new(raw).is_err(), "account: {raw:?}");
        }
    }
}
