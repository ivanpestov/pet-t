//! Скрытый режим автодополнения Account для shell: кандидаты из вывода
//! `ledger accounts`, отфильтрованные по префиксу текущего слова.

use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Stdio};

use crate::adapters::args::CompletionRequest;

/// Выполняет режим автодополнения: печатает кандидатов в stdout. Код
/// выхода выбирает бинарник при композиции. Сценарий Таймера, Ctrl+C,
/// монитор сна и уведомления не затрагиваются.
pub fn run(request: &CompletionRequest) {
    if request.previous.as_encoded_bytes() == b"t" {
        print_candidates(&request.current);
    }
}

/// Печатает по строке на кандидата: stdout `ledger accounts` без
/// изменений, только строки с префиксом `current` (байтовое,
/// регистрозависимое сравнение; пустое слово — весь список). Любой сбой
/// дочернего процесса даёт пустой stdout; его stderr подавляется.
fn print_candidates(current: &OsStr) {
    let current = current.as_encoded_bytes();
    let Ok(output) = Command::new("ledger")
        .arg("accounts")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    if output.stdout.is_empty() {
        return;
    }
    // Завершающий перевод строки завершает последнюю строку, но пустой
    // строкой не считается; пустые строки внутри вывода сохраняются.
    let body = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
    let mut stdout = std::io::stdout().lock();
    for line in body.split(|byte| *byte == b'\n') {
        if !line.starts_with(current) {
            continue;
        }
        if stdout
            .write_all(line)
            .and_then(|()| stdout.write_all(b"\n"))
            .is_err()
        {
            break;
        }
    }
}
