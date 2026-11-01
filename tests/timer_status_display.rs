use std::{
    env, fs,
    io::Read,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

struct TestDir(PathBuf);

impl TestDir {
    fn new(label: &str) -> Self {
        let path = env::temp_dir().join(format!("t-timer-status-{label}-{}", std::process::id()));
        fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }

    fn command(&self, minutes: u64) -> Command {
        let bin = self.0.join("bin");
        fs::create_dir_all(&bin).expect("create stub directory");
        write_executable(
            &bin.join("date"),
            "#!/bin/sh\ncase \"$1\" in\n  +%H:%M) printf '12:34\\n' ;;\n  *) printf '2026/11/01 12:34:56\\n' ;;\nesac\n",
        );
        write_executable(&bin.join("notify-send"), "#!/bin/sh\nexit 0\n");

        let inherited_path = env::var_os("PATH").unwrap_or_default();
        let path = env::join_paths(
            std::iter::once(bin.into_os_string())
                .chain(env::split_paths(&inherited_path).map(|path| path.into_os_string())),
        )
        .expect("construct PATH");
        let mut command = Command::new(env!("CARGO_BIN_EXE_t"));
        command
            .arg("work")
            .arg(minutes.to_string())
            .env("LEDGER_FILE", self.0.join("time.ledger"))
            .env("PATH", path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        command
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Timer {
    child: Child,
    chunks: Receiver<String>,
    stdout: String,
}

impl Timer {
    fn start(command: &mut Command) -> Self {
        let mut child = command.spawn().expect("start timer");
        let mut stdout = child.stdout.take().expect("capture stdout");
        let (chunk_tx, chunks) = mpsc::channel();
        thread::spawn(move || {
            let mut buffer = [0; 1024];
            while let Ok(count) = stdout.read(&mut buffer) {
                if count == 0 {
                    break;
                }
                if chunk_tx
                    .send(String::from_utf8_lossy(&buffer[..count]).into_owned())
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            child,
            chunks,
            stdout: String::new(),
        }
    }

    fn wait_for_output(&mut self, expected: &str, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while !self.stdout.contains(expected) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            match self.chunks.recv_timeout(remaining) {
                Ok(chunk) => self.stdout.push_str(&chunk),
                Err(error) => panic!(
                    "timed out waiting for {expected:?}: {error}; stdout: {:?}",
                    self.stdout
                ),
            }
        }
    }

    fn interrupt(mut self) -> (ExitStatus, String) {
        let status = Command::new("kill")
            .arg("-INT")
            .arg(self.child.id().to_string())
            .status()
            .expect("send SIGINT");
        assert!(status.success());
        let status = self.child.wait().expect("wait for timer");
        while let Ok(chunk) = self.chunks.recv() {
            self.stdout.push_str(&chunk);
        }
        (status, self.stdout.clone())
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write command stub");
    let mut permissions = fs::metadata(path).expect("stat command stub").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make command stub executable");
}

#[test]
fn initial_status_contains_fixed_boundaries_and_duration_over_an_hour() {
    let dir = TestDir::new("initial");
    let mut timer = Timer::start(&mut dir.command(90));

    timer.wait_for_output("\r12:34 => 01:30 => 14:04", Duration::from_secs(5));
    let (status, stdout) = timer.interrupt();

    assert_eq!(status.code(), Some(130));
    assert_eq!(stdout, "\r12:34 => 01:30 => 14:04\n");
    assert!(!stdout.contains("work"));
}

#[test]
fn minute_update_rounds_remaining_time_up_without_changing_boundaries() {
    let dir = TestDir::new("minute-update");
    let mut timer = Timer::start(&mut dir.command(2));

    timer.wait_for_output("\r12:34 => 00:02 => 12:36", Duration::from_secs(5));
    timer.wait_for_output("\r12:34 => 00:01 => 12:36", Duration::from_secs(65));
    let (status, stdout) = timer.interrupt();

    assert_eq!(status.code(), Some(130));
    assert_eq!(
        stdout,
        "\r12:34 => 00:02 => 12:36\r12:34 => 00:01 => 12:36\n"
    );
}
