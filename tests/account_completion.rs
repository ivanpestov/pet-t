use std::{
    env,
    ffi::{OsStr, OsString},
    fs,
    os::unix::{
        ffi::OsStringExt,
        fs::{PermissionsExt, symlink},
    },
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

/// Account list printed by the ledger stub; printf interprets the
/// escapes, so each name lands on its own line.
const STUB_ACCOUNTS: &str = "work\\nwork:call\\npersonal\\nWork\\n";

const COMPLETION_SCRIPT: &str = r#"# bash completion for t
_t_completion() {
    local line="${COMP_LINE:0:COMP_POINT}"
    local current="${line##*[[:space:]]}"
    local before="${line:0:${#line}-${#current}}"
    before="${before%"${before##*[![:space:]]}"}"
    local previous="${before##*[[:space:]]}"
    COMPREPLY=()
    mapfile -t COMPREPLY < <(command t --complete "$current" "$previous")
    if [[ "$current" == *:* ]]; then
        local prefix="${current%:*}:"
        local index
        for index in "${!COMPREPLY[@]}"; do
            COMPREPLY[index]="${COMPREPLY[index]#"$prefix"}"
        done
    fi
}
complete -F _t_completion t
"#;

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
    fn complete(
        &self,
        current: impl AsRef<OsStr>,
        previous: impl AsRef<OsStr>,
        ledger_file: &Path,
    ) -> Output {
        self.complete_with_env(current, previous, Some(ledger_file), &self.stub_path())
    }

    /// Runs `t --complete <current> <previous>` with an explicit PATH and
    /// an optional LEDGER_FILE (removed from the child environment when
    /// absent).
    fn complete_with_env(
        &self,
        current: impl AsRef<OsStr>,
        previous: impl AsRef<OsStr>,
        ledger_file: Option<&Path>,
        path: &OsStr,
    ) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_t"));
        command
            .arg("--complete")
            .arg(current.as_ref())
            .arg(previous.as_ref())
            .env("PATH", path)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match ledger_file {
            Some(ledger_file) => {
                command.env("LEDGER_FILE", ledger_file);
            }
            None => {
                command.env_remove("LEDGER_FILE");
            }
        }
        command.output().expect("run completion mode")
    }

    /// PATH with the stub directory prepended to the inherited value.
    fn stub_path(&self) -> OsString {
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

/// TestDir with an empty ledger file created inside it.
fn dir_with_ledger(label: &str) -> (TestDir, PathBuf) {
    let dir = TestDir::new(label);
    let ledger = dir.ledger_file();
    fs::write(&ledger, "").expect("create ledger file");
    (dir, ledger)
}

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).expect("write command stub");
    let mut permissions = fs::metadata(path).expect("stat command stub").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("make command stub executable");
}

#[test]
fn bare_complete_prints_the_bash_completion_script() {
    let output = Command::new(env!("CARGO_BIN_EXE_t"))
        .arg("--complete")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run completion script mode");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf8 stdout"),
        COMPLETION_SCRIPT
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn completion_script_completes_segments_after_a_colon() {
    let dir = TestDir::new("colon");
    let bin = dir.path.join("bin");
    symlink(env!("CARGO_BIN_EXE_t"), bin.join("t")).expect("link t binary");
    write_executable(
        &bin.join("ledger"),
        "#!/bin/sh\nprintf 'work\\nmath:equations\\nmath:tests\\n'\n",
    );
    let script = dir.path.join("completion.bash");
    let output = Command::new(env!("CARGO_BIN_EXE_t"))
        .arg("--complete")
        .output()
        .expect("print completion script");
    fs::write(&script, &output.stdout).expect("write completion script");

    let inherited_path = env::var_os("PATH").unwrap_or_default();
    let path = env::join_paths(
        std::iter::once(bin.into_os_string())
            .chain(env::split_paths(&inherited_path).map(|path| path.into_os_string())),
    )
    .expect("construct PATH");
    let output = Command::new("bash")
        .arg("-c")
        .arg(
            "source \"$1\"; \
             COMP_LINE='t math:'; \
             COMP_POINT=${#COMP_LINE}; \
             _t_completion; \
             printf '%s\\n' \"${COMPREPLY[@]}\"",
        )
        .arg("bash")
        .arg(&script)
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run completion function");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf8 stdout"),
        "equations\ntests\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn empty_current_word_lists_all_accounts() {
    let (dir, ledger) = dir_with_ledger("all");

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
    let (dir, ledger) = dir_with_ledger("prefix");

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
    let (dir, ledger) = dir_with_ledger("case");

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
    let (dir, ledger) = dir_with_ledger("second-arg");

    let output = dir.complete("", "5", &ledger);

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_ledger_yields_empty_output() {
    let (dir, ledger) = dir_with_ledger("no-ledger");
    let empty_bin = dir.path.join("empty-bin");
    fs::create_dir_all(&empty_bin).expect("create empty bin directory");

    let output = dir.complete_with_env("wo", "t", Some(&ledger), empty_bin.as_os_str());

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert!(output.stderr.is_empty());
}

#[test]
fn missing_ledger_file_yields_empty_output() {
    let dir = TestDir::new("no-ledger-file");
    let stub_bin = dir.path.join("bin");

    let output = dir.complete_with_env("", "t", None, stub_bin.as_os_str());

    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert!(output.stderr.is_empty());
}

#[test]
fn non_utf8_current_word_does_not_panic() {
    let (dir, ledger) = dir_with_ledger("non-utf8");
    let current = OsString::from_vec(vec![b'w', b'o', 0xff]);

    let output = dir.complete(&current, "t", &ledger);

    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty(), "stdout: {:?}", output.stdout);
    assert!(output.stderr.is_empty());
}

#[test]
fn byte_built_prefix_still_filters_candidates() {
    let (dir, ledger) = dir_with_ledger("byte-prefix");
    let current = OsString::from_vec(b"wo".to_vec());

    let output = dir.complete(&current, "t", &ledger);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf8 stdout"),
        "work\nwork:call\n"
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn empty_line_in_ledger_output_is_preserved() {
    let (dir, ledger) = dir_with_ledger("empty-line");
    write_executable(
        &dir.path.join("bin").join("ledger"),
        "#!/bin/sh\nprintf 'work\\n\\npersonal\\n'",
    );

    let output = dir.complete("", "t", &ledger);

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).expect("utf8 stdout"),
        "work\n\npersonal\n"
    );
    assert!(output.stderr.is_empty());
}
