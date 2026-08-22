use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root")
}

fn free_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("reserve loopback port")
        .local_addr()
        .unwrap()
        .port()
}

fn wait_port(port: u16, deadline: Duration, diagnostics: Option<&Path>) {
    let started = Instant::now();
    while started.elapsed() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let detail = diagnostics
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_default();
    panic!("timed out waiting for loopback port {port}: {detail}");
}

struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn stable_and_latest_exact_requests_replay_through_native_engine() {
    if std::env::var_os("CLAUDE_CODE_COMPAT_SKIP").is_some() {
        return;
    }
    let root = repo_root();
    let temp = std::env::temp_dir().join(format!(
        "claude-code-runtime-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(temp.join("engine-spool")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            temp.join("engine-spool"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
    }
    let mock_port = free_port();
    let engine_port = free_port();
    let log = temp.join("mock.log");
    let mock = Command::new("python3")
        .arg(root.join("tests/mock_upstream.py"))
        .arg(mock_port.to_string())
        .env("SRV_LOG", &log)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start mock upstream");
    let _mock = ChildGuard(mock);
    wait_port(mock_port, Duration::from_secs(10), None);

    let bin = PathBuf::from(env!("CARGO_BIN_EXE_claude-api"));
    let token_file = temp.join("token");
    std::fs::write(&token_file, "faketokenaaaaaaaaaaaa\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token_file, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let common = |command: &mut Command| {
        command
            .env("SUB_CFG_DIR", &temp)
            .env("CLAUDE_API_PROVIDER", "anthropic")
            .env("CLAUDE_API_BODY_SPOOL_ROOT", temp.join("engine-spool"))
            .env("CLAUDE_API_HOST", "127.0.0.1")
            .env("CLAUDE_API_PORT", engine_port.to_string())
            .env(
                "CLAUDE_API_KEYS",
                "claude-code-runtime-key-00000000000000000000",
            )
            .env("CLAUDE_API_BILLING", "0")
            .env("CLAUDE_API_POLL", "0")
            .env("CLAUDE_API_UPSTREAM", format!("http://127.0.0.1:{mock_port}"))
            .env("CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM", "1");
    };
    let mut add = Command::new(&bin);
    common(&mut add);
    assert!(add
        .args(["sub", "add-file", "sub-a@test.io", "--token-file"])
        .arg(&token_file)
        .status()
        .unwrap()
        .success());
    let mut plan = Command::new(&bin);
    common(&mut plan);
    assert!(plan
        .args(["sub", "set-plan", "sub-a@test.io", "max20"])
        .status()
        .unwrap()
        .success());
    let engine_stdout = std::fs::File::create(temp.join("engine.stdout")).unwrap();
    let engine_stderr = std::fs::File::create(temp.join("engine.stderr")).unwrap();
    let mut serve = Command::new(&bin);
    common(&mut serve);
    let engine = serve
        .arg("serve")
        .stdout(Stdio::from(engine_stdout))
        .stderr(Stdio::from(engine_stderr))
        .spawn()
        .expect("start engine");
    let _engine = ChildGuard(engine);
    let base = format!("http://127.0.0.1:{engine_port}");
    wait_port(
        engine_port,
        Duration::from_secs(15),
        Some(&temp.join("engine.stderr")),
    );

    let status = Command::new("bash")
        .arg(root.join("tests/claude_code_compat_matrix.sh"))
        .env("CLAUDE_CODE_COMPAT_RUNTIME_BASE_URL", &base)
        .env(
            "CLAUDE_CODE_COMPAT_RUNTIME_API_KEY",
            "claude-code-runtime-key-00000000000000000000",
        )
        .env("CLAUDE_CODE_COMPAT_CACHE_ROOT", temp.join("client-cache"))
        .status()
        .expect("run exact Claude Code matrix");
    assert!(status.success());
    let _ = std::fs::remove_dir_all(&temp);
}
