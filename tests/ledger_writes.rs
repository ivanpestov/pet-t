use std::{
    env, fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant},
};

const TIMESTAMP: &str = "2026/11/01 12:34:56";
static NEXT_TEMP_DIR: AtomicU64 = AtomicU64::new(0);

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new() -> Self {
        let sequence = NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed);
        let path =
            env::temp_dir().join(format!("t-ledger-writes-{}-{sequence}", std::process::id()));
        fs::create_dir_all(&path).expect("create test directory");
        Self { path }
    }

    fn command(&self, ledger: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_t"));
        self.configure(&mut command, ledger);
        command
    }

    fn configure(&self, command: &mut Command, ledger: &Path) {
        let bin = self.path.join("bin");
        fs::create_dir_all(&bin).expect("create stub directory");
        write_executable(
            &bin.join("date"),
            &format!(
                "#!/bin/sh\ncase \"$1\" in\n  +%H:%M)\n    count=0\n    [ -f \"$DATE_COUNTER\" ] && count=$(cat \"$DATE_COUNTER\")\n    case \"$count\" in\n      0) printf '12:34\\n' ;;\n      *) printf '12:36\\n' ;;\n    esac\n    printf '%s' \"$((count + 1))\" > \"$DATE_COUNTER\"\n    ;;\n  *) printf '%s\\n' '{TIMESTAMP}' ;;\nesac\n"
            ),
        );
        write_executable(
            &bin.join("notify-send"),
            "#!/bin/sh\nprintf 'notify-send: %s\\n' \"$*\" >&2\nprintf '%s\\n' \"$*\" >> \"$NOTIFY_LOG\"\n",
        );

        let inherited_path = env::var_os("PATH").unwrap_or_default();
        let path = env::join_paths(
            std::iter::once(bin.into_os_string())
                .chain(env::split_paths(&inherited_path).map(|p| p.into_os_string())),
        )
        .expect("construct PATH");
        command
            .arg("work")
            .arg("1")
            .env("LEDGER_FILE", ledger)
            .env("DATE_COUNTER", self.path.join("date-count"))
            .env("NOTIFY_LOG", self.path.join("notify.log"))
            .env("PATH", path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct ChildGuard(Child);

impl ChildGuard {
    fn wait_for_start(&mut self, ledger: &Path) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if fs::read_to_string(ledger).is_ok_and(|contents| contents.starts_with("i ")) {
                return;
            }
            if let Some(status) = self.0.try_wait().expect("poll timer") {
                panic!("timer exited before its initial ledger write: {status}");
            }
            assert!(Instant::now() < deadline, "initial ledger write timed out");
            thread::sleep(Duration::from_millis(20));
        }
    }

    fn finish(mut self) -> (ExitStatus, String, String) {
        let status = self.0.wait().expect("wait for timer");
        let mut stdout = String::new();
        self.0
            .stdout
            .take()
            .expect("capture stdout")
            .read_to_string(&mut stdout)
            .expect("read stdout");
        let mut stderr = String::new();
        self.0
            .stderr
            .take()
            .expect("capture stderr")
            .read_to_string(&mut stderr)
            .expect("read stderr");
        (status, stdout, stderr)
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write command stub");
    let mut permissions = fs::metadata(path).expect("stat command stub").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make command stub executable");
}

fn run(command: &mut Command) -> ChildGuard {
    ChildGuard(command.spawn().expect("start timer"))
}

#[test]
fn natural_completion_writes_period_and_notifies() {
    let dir = TestDir::new();
    let ledger = dir.path.join("time.ledger");
    let output = dir
        .command(&ledger)
        .output()
        .expect("run timer to completion");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(&ledger).expect("read ledger"),
        format!("i {TIMESTAMP} work\no {TIMESTAMP}\n")
    );
    assert_eq!(
        fs::read_to_string(dir.path.join("notify.log")).expect("read notification log"),
        "Timer finished Time is up\n"
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "\r12:34 => 00:01 => 12:35\r12:34 => 00:00 => 12:36\n"
    );
}

#[test]
fn interrupt_closes_period_without_notification() {
    let dir = TestDir::new();
    let ledger = dir.path.join("time.ledger");
    let mut child = run(&mut dir.command(&ledger));
    child.wait_for_start(&ledger);

    let status = Command::new("kill")
        .arg("-INT")
        .arg(child.0.id().to_string())
        .status()
        .expect("send SIGINT");
    assert!(status.success());
    let (status, _stdout, stderr) = child.finish();

    assert_eq!(status.code(), Some(130), "stderr: {stderr}");
    assert_eq!(
        fs::read_to_string(&ledger).expect("read ledger"),
        format!("i {TIMESTAMP} work\no {TIMESTAMP}\n")
    );
    assert!(!dir.path.join("notify.log").exists());
}

#[test]
fn final_write_failure_is_reported_and_still_notifies() {
    let dir = TestDir::new();
    let ledger = dir.path.join("time.ledger");
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg("trap '' XFSZ; exec \"$1\" work 1")
        .arg("bash")
        .arg(env!("CARGO_BIN_EXE_t"));
    dir.configure(&mut command, &ledger);
    let mut child = run(&mut command);
    child.wait_for_start(&ledger);

    let status = Command::new("prlimit")
        .arg("--pid")
        .arg(child.0.id().to_string())
        .arg("--fsize=0:0")
        .status()
        .expect("limit ledger writes");
    assert!(status.success());
    let (status, _stdout, stderr) = child.finish();

    assert_eq!(status.code(), Some(1), "stderr: {stderr}");
    assert!(
        stderr.contains(&format!("cannot write {}:", ledger.display())),
        "stderr: {stderr}"
    );
    assert_eq!(
        fs::read_to_string(&ledger).expect("read ledger"),
        format!("i {TIMESTAMP} work\n")
    );
    assert!(
        stderr.contains("notify-send: Timer finished Time is up\n"),
        "stderr: {stderr}"
    );
}
