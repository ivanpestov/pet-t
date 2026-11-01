use std::{
    fs,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const USAGE: &str = "Usage: t <minutes> | t <account> <minutes>";

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "t-account-validation-{label}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn rejects_invalid_accounts_before_touching_ledger() {
    let invalid_accounts = [
        "fn",
        "work:async",
        "Self",
        "_",
        "work:_",
        "r#type",
        "work:r#project",
        "",
        ":work",
        "work:",
        "work::project",
        "1work",
        "work:2project",
        "work project",
        "work:project alpha",
        "work\nproject",
        "work\rproject",
        "work-project",
        "work:project!",
    ];

    for account in invalid_accounts {
        let dir = TestDir::new("invalid");
        let ledger = dir.path().join("time.ledger");
        let original = "i 2026/01/01 00:00:00 previous\n";
        fs::write(&ledger, original).unwrap();

        let output = Command::new(env!("CARGO_BIN_EXE_t"))
            .args([account, "1"])
            .env("LEDGER_FILE", &ledger)
            .output()
            .unwrap();

        assert_eq!(output.status.code(), Some(2), "account: {account:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(USAGE),
            "account: {account:?}, stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(&ledger).unwrap(),
            original,
            "account: {account:?}"
        );

        let missing_ledger = dir.path().join("missing.ledger");
        let output = Command::new(env!("CARGO_BIN_EXE_t"))
            .args([account, "1"])
            .env("LEDGER_FILE", &missing_ledger)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2), "account: {account:?}");
        assert!(!missing_ledger.exists(), "account: {account:?}");
    }
}

#[test]
fn writes_valid_accounts_in_full_without_a_payee() {
    let valid_accounts = [
        "work",
        "work:project",
        "_work:project_2",
        "__:selfish",
        "async_:gen2",
        "работа:проект2",
        "Work:work",
        "e\u{301}:cafe\u{301}",
        "δ2:任务_3",
    ];

    for account in valid_accounts {
        let dir = TestDir::new("valid");
        let ledger = dir.path().join("time.ledger");
        let mut child = Command::new(env!("CARGO_BIN_EXE_t"))
            .args([account, "1"])
            .env("LEDGER_FILE", &ledger)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        wait_for_ledger_start(&mut child, &ledger);
        let initial = fs::read_to_string(&ledger).unwrap();
        let status = interrupt(&mut child);

        assert_eq!(status.code(), Some(130), "account: {account:?}");
        assert!(initial.starts_with("i "), "account: {account:?}");
        assert!(
            initial.ends_with(&format!(" {account}\n")),
            "account: {account:?}, initial entry: {initial:?}"
        );
        assert_eq!(initial.lines().count(), 1, "account: {account:?}");

        let finished = fs::read_to_string(&ledger).unwrap();
        assert!(finished.starts_with(&initial), "account: {account:?}");
        assert_eq!(finished.lines().count(), 2, "account: {account:?}");
        assert!(
            finished.lines().nth(1).unwrap().starts_with("o "),
            "account: {account:?}"
        );
    }
}

fn wait_for_ledger_start(child: &mut Child, ledger: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if fs::read_to_string(ledger).is_ok_and(|contents| contents.starts_with("i ")) {
            return;
        }
        if let Some(status) = child.try_wait().unwrap() {
            panic!("timer exited before writing the initial entry: {status}");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("timed out waiting for the initial ledger entry");
        }
        thread::sleep(Duration::from_millis(25));
    }
}

fn interrupt(child: &mut Child) -> ExitStatus {
    let status = Command::new("kill")
        .arg("-INT")
        .arg(child.id().to_string())
        .status()
        .unwrap();
    assert!(status.success());

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("timed out waiting for the timer to stop after SIGINT");
        }
        thread::sleep(Duration::from_millis(25));
    }
}
