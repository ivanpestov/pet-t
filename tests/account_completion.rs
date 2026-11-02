use std::{
    env, fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// Account list printed by the ledger stub; printf interprets the
/// escapes, so each name lands on its own line.
const STUB_ACCOUNTS: &str = "work\\nwork:call\\npersonal\\nWork\\n";

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(label: &str) -> Self {
        let path = env::temp_dir().join(format!(
            "t-account-completion-{label}-{}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create test directory");
        let dir = Self { path };
        dir.write_ledger_stub();
        dir
    }

    fn ledger_file(&self) -> PathBuf {
        self.path.join("time.ledger")
    }

    /// Runs `t --complete <current> <previous>` with the stub ledger on
    /// PATH and LEDGER_FILE set to `ledger_file`.
    fn complete(&self, current: &str, previous: &str, ledger_file: &Path) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_t"));
        command
            .arg("--complete")
            .arg(current)
            .arg(previous)
            .env("LEDGER_FILE", ledger_file)
            .env("PATH", self.stub_path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command.output().expect("run completion mode")
    }

    /// PATH with the stub directory prepended to the inherited value.
    fn stub_path(&self) -> std::ffi::OsString {
        let bin = self.path.join("bin");
        let inherited = env::var_os("PATH").unwrap_or_default();
        env::join_paths(
            std::iter::once(bin.into_os_string())
                .chain(env::split_paths(&inherited).map(|path| path.into_os_string())),
        )
        .expect("construct PATH")
    }

    /// Stub `ledger accounts`: prints the fixed account list when
    /// LEDGER_FILE points at an existing file, fails to stderr otherwise.
    fn write_ledger_stub(&self) {
        let bin = self.path.join("bin");
        fs::create_dir_all(&bin).expect("create stub directory");
        write_executable(
            &bin.join("ledger"),
            &format!(
                "#!/bin/sh\nif [ \"$1\" != accounts ]; then\n  echo \"ledger: unknown command: $1\" >&2\n  exit 1\nfi\nif [ -z \"$LEDGER_FILE\" ] || [ ! -f \"$LEDGER_FILE\" ]; then\n  echo 'ledger: cannot find ledger file' >&2\n  exit 1\nfi\nprintf '{STUB_ACCOUNTS}'"
            ),
        );
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write command stub");
    let mut permissions = fs::metadata(path).expect("stat command stub").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make command stub executable");
}

#[test]
fn empty_current_word_lists_all_accounts() {
    let dir = TestDir::new("all");
    let ledger = dir.ledger_file();
    fs::write(&ledger, "").expect("create ledger file");

    let output = dir.complete("", "t", &ledger);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf8 stdout"),
        "work\nwork:call\npersonal\nWork\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn current_word_filters_candidates_by_prefix() {
    let dir = TestDir::new("prefix");
    let ledger = dir.ledger_file();
    fs::write(&ledger, "").expect("create ledger file");

    let output = dir.complete("wo", "t", &ledger);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf8 stdout"),
        "work\nwork:call\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn prefix_match_is_case_sensitive() {
    let dir = TestDir::new("case");
    let ledger = dir.ledger_file();
    fs::write(&ledger, "").expect("create ledger file");

    let output = dir.complete("Wo", "t", &ledger);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf8 stdout"),
        "Work\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn other_previous_word_completes_to_nothing() {
    let dir = TestDir::new("second-arg");
    let ledger = dir.ledger_file();
    fs::write(&ledger, "").expect("create ledger file");

    let output = dir.complete("", "5", &ledger);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_ledger_yields_empty_output() {
    let dir = TestDir::new("no-ledger");
    let ledger = dir.ledger_file();
    fs::write(&ledger, "").expect("create ledger file");
    let empty_bin = dir.path.join("empty-bin");
    fs::create_dir_all(&empty_bin).expect("create empty bin directory");

    let mut command = Command::new(env!("CARGO_BIN_EXE_t"));
    command
        .arg("--complete")
        .arg("wo")
        .arg("t")
        .env("LEDGER_FILE", &ledger)
        .env("PATH", &empty_bin)
        .stdin(Stdio::null());
    let output = command.output().expect("run completion mode");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_ledger_file_yields_empty_output() {
    let dir = TestDir::new("no-ledger-file");
    let mut command = Command::new(env!("CARGO_BIN_EXE_t"));
    command
        .arg("--complete")
        .arg("")
        .arg("t")
        .env_remove("LEDGER_FILE")
        .env("PATH", dir.path.join("bin"))
        .stdin(Stdio::null());
    let output = command.output().expect("run completion mode");

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert!(output.stderr.is_empty());
}
