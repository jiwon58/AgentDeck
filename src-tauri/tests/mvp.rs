use agent_deck::{
    adapter,
    model::{Agent, Launch, Preferences, Router},
    registry, router,
    storage::{Store, Table},
    terminal::Terminal,
};
use portable_pty::CommandBuilder;
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    path::Path,
    thread,
    time::{Duration, Instant},
};

const MOCK: &str = env!("CARGO_BIN_EXE_mock-agent");

#[test]
fn anthropic_catalog_paginates_and_model_test_requires_text() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind model API");
    let mut router = profile(listener.local_addr().expect("address").port());
    router.anthropic_path = Some("/claude/v1/".into());
    router.authentication = "bearer".into();
    let server = thread::spawn(move || {
        for (route, reply) in [
            (
                "GET /claude/v1/models ",
                r#"{"data":[{"id":"model-b"}],"has_more":true,"last_id":"model-b"}"#,
            ),
            (
                "GET /claude/v1/models?after_id=model-b ",
                r#"{"data":[{"id":"model-a"},{"id":"model-b"}],"has_more":false}"#,
            ),
            (
                "POST /claude/v1/messages ",
                r#"{"content":[{"type":"text","text":"OK"}]}"#,
            ),
            (
                "POST /claude/v1/messages ",
                r#"{"content":[{"type":"thinking","thinking":"internal"}]}"#,
            ),
        ] {
            let (mut stream, _) = listener.accept().expect("accept");
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .expect("timeout");
            let mut reader = BufReader::new(&mut stream);
            let mut line = String::new();
            reader.read_line(&mut line).expect("request line");
            assert!(line.starts_with(route), "{line}");
            let mut headers = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).expect("header") == 0 || line == "\r\n" {
                    break;
                }
                headers.push_str(&line.to_ascii_lowercase());
            }
            assert!(headers.contains("authorization: bearer fixture"));
            assert!(headers.contains("anthropic-version: 2023-06-01"));
            if route.starts_with("POST") {
                let len: usize = headers
                    .lines()
                    .find_map(|line| line.strip_prefix("content-length: "))
                    .expect("length")
                    .parse()
                    .expect("number");
                let mut body = vec![0; len];
                reader.read_exact(&mut body).expect("request body");
                let body: serde_json::Value = serde_json::from_slice(&body).expect("JSON request");
                assert_eq!(body["model"], "model-a");
                assert_eq!(body["max_tokens"], 64);
                assert_eq!(body["messages"][0]["content"], router::MODEL_TEST_PROMPT);
            }
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            )
            .expect("response");
        }
    });
    let api = router::ModelProtocol::Anthropic;
    let catalog = router::list_models(&router, "fixture", api).expect("catalog");
    assert!(catalog.complete);
    assert_eq!(catalog.models, ["model-a", "model-b"]);
    assert!(
        router::test_model(&router, "fixture", api, "model-a")
            .expect("text")
            .responded
    );
    assert!(
        !router::test_model(&router, "fixture", api, "model-a")
            .expect("no text")
            .responded
    );
    server.join().expect("model API");
}

fn copy_mock(dir: &Path, file: &str) -> String {
    fs::create_dir_all(dir).expect("fixture directory");
    let path = dir.join(file);
    fs::copy(MOCK, &path).expect("copy mock");
    path.to_string_lossy().into_owned()
}

#[test]
fn registry_scans_separate_installs_and_rechecks_identity() {
    let root = tempfile::tempdir().expect("temporary directory");
    let a = copy_mock(
        &root
            .path()
            .join("임의 경로")
            .join("a".repeat(100))
            .join("b".repeat(100)),
        "gjc.exe",
    );
    let b = copy_mock(&root.path().join("custom nightly folder"), "gjc.exe");
    let launcher = root.path().join("gjc.cmd");
    fs::write(&launcher, "@echo off\r\n").expect("create script fixture");
    let found = registry::scan(Agent::Gajae, root.path().to_str().expect("path")).expect("scan");
    assert_eq!(found.candidates.len(), 3);
    assert_eq!(found.candidates[2].launch_type, "requires_interpreter");
    assert!(registry::inspect(launcher.to_str().expect("launcher")).is_err());
    let a = registry::inspect(&a).expect("first candidate");
    let b = registry::inspect(&b).expect("second candidate");
    assert_ne!(a.path, b.path);
    assert_eq!(a.sha256, b.sha256);
    assert_eq!(
        registry::version(&a, &[]).expect("version").as_deref(),
        Some("1.2.3")
    );
    assert_eq!(
        registry::version(&b, &["--mock-version=2.0.0".into()])
            .expect("second version")
            .as_deref(),
        Some("2.0.0")
    );
    assert_eq!(
        registry::version(&b, &["--bad-version".into()]).expect("invalid version"),
        None
    );
    fs::OpenOptions::new()
        .append(true)
        .open(&b.path)
        .expect("open file")
        .write_all(b"changed")
        .expect("change file");
    assert!(registry::verify(&b)
        .expect_err("changed executable must be refused")
        .contains("changed"));
    fs::remove_file(&a.path).expect("remove first");
    assert!(registry::verify(&a).is_err());
}

#[test]
fn version_probe_times_out() {
    let candidate = registry::inspect(MOCK).expect("mock executable");
    let start = Instant::now();
    assert_eq!(
        registry::version(&candidate, &["--slow".into()]).expect("timeout"),
        None
    );
    assert!(start.elapsed() < Duration::from_secs(8));
}

fn collect(terminal: &Terminal, id: &str, bytes: &mut Vec<u8>, until: impl Fn(&[u8]) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(12);
    while Instant::now() < deadline {
        for event in terminal.drain(id).expect("drain terminal") {
            if event.event_type == "terminal_output" {
                bytes.extend(
                    serde_json::from_value::<Vec<u8>>(event.payload).expect("terminal bytes"),
                );
            }
        }
        #[cfg(windows)]
        if bytes.ends_with(b"\x1b[6n") {
            // ConPTY inherits the cursor position. A real xterm answers this DSR query.
            terminal
                .write(id, "\x1b[1;1R".into())
                .expect("reply to ConPTY cursor query");
            bytes.push(b'\n');
        }
        if until(bytes) {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!(
        "Timed out waiting for {id}: {}",
        String::from_utf8_lossy(bytes)
    );
}

#[test]
fn three_ptys_accept_input_resize_and_exit_independently() {
    let root = tempfile::tempdir().expect("temporary directory");
    let terminal = Terminal::default();
    let mut output = [Vec::new(), Vec::new(), Vec::new()];
    for n in 0..3 {
        let cwd = root.path().join(format!("한글 folder {n}"));
        fs::create_dir(&cwd).expect("mkdir fixture");
        let mut command = CommandBuilder::new(MOCK);
        command.cwd(cwd);
        terminal
            .start(n.to_string(), command, (90, 28), |_| {})
            .expect("start PTY");
    }
    assert_eq!(terminal.active_count().expect("active count"), 3);
    for (n, buffer) in output.iter_mut().enumerate() {
        let id = &n.to_string();
        collect(&terminal, id, buffer, |s| {
            String::from_utf8_lossy(s).contains(&format!("folder {n}"))
        });
        assert!(buffer.contains(&0x1b));
        terminal.resize(id, 96, 32).expect("resize PTY");
        terminal
            .write(id, format!("input-{n}\r"))
            .expect("write input");
        collect(&terminal, id, buffer, |s| {
            String::from_utf8_lossy(s).contains(&format!("echo: input-{n}"))
        });
    }
    terminal
        .write("0", "stream 1500\r".into())
        .expect("stream command");
    collect(&terminal, "0", &mut output[0], |s| {
        String::from_utf8_lossy(s).contains("stream 1499")
    });
    terminal
        .write("1", "exit 7\r".into())
        .expect("exit command");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(event) = terminal
            .drain("1")
            .expect("drain events")
            .into_iter()
            .find(|event| event.event_type == "terminal_exited")
        {
            assert_eq!(event.payload["code"], 7);
            break;
        }
        assert!(Instant::now() < deadline, "terminal did not exit");
        thread::sleep(Duration::from_millis(30));
    }
    assert_eq!(terminal.status("1").expect("terminal status"), "exited");
    assert_eq!(terminal.status("2").expect("terminal status"), "running");
    terminal.close("0").expect("stop first tab");
    terminal.shutdown().expect("stop all tabs");
    assert_eq!(terminal.active_count().expect("count after shutdown"), 0);
}

fn profile(port: u16) -> Router {
    Router {
        id: "mock".into(),
        name: "Mock router".into(),
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port,
        openai_path: Some("/v1".into()),
        anthropic_path: None,
        model: "model-a".into(),
        authentication: "none".into(),
        enabled: true,
        allow_remote: false,
        created_at: 1,
        updated_at: 1,
    }
}

fn mock_router(status: u16, body: &'static str, delay: Duration) -> (u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind mock router");
    let port = listener.local_addr().expect("local address").port();
    let server = thread::spawn(move || {
        let (_, _) = listener.accept().expect("TCP check");
        let (mut stream, _) = listener.accept().expect("HTTP check");
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .expect("read timeout");
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("request line");
        assert!(line.starts_with("GET /v1/models "));
        loop {
            line.clear();
            if reader.read_line(&mut line).expect("header") == 0 || line == "\r\n" {
                break;
            }
        }
        thread::sleep(delay);
        let _ = write!(
            stream,
            "HTTP/1.1 {status} Mock\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
    });
    (port, server)
}

#[test]
fn router_reports_model_list_http_errors_and_timeout() {
    for (status, body, expected, delay) in [
        (
            200,
            r#"{"data":[{"id":"model-a"}]}"#,
            "Model list available",
            Duration::ZERO,
        ),
        (401, "", "Authentication required", Duration::ZERO),
        (404, "", "Path not found", Duration::ZERO),
        (200, "{}", "HTTP timeout", Duration::from_millis(3400)),
    ] {
        let (port, server) = mock_router(status, body, delay);
        let result = serde_json::to_value(router::test(&profile(port), "").expect("router probe"))
            .expect("probe JSON");
        assert_eq!(result["tcp"], "connected");
        assert!(
            result["result"]
                .as_str()
                .expect("result text")
                .contains(expected),
            "{result}"
        );
        if delay.is_zero() {
            assert_eq!(result["http"], status);
        }
        server.join().expect("mock server");
    }
    let unused = TcpListener::bind(("127.0.0.1", 0)).expect("temporary port");
    let port = unused.local_addr().expect("local address").port();
    drop(unused);
    let result = serde_json::to_value(router::test(&profile(port), "").expect("closed port probe"))
        .expect("probe JSON");
    assert!(
        matches!(result["tcp"].as_str(), Some("refused" | "timeout")),
        "{result}"
    );
}

#[test]
fn claude_requires_anthropic_and_only_new_bindings_use_updated_port() {
    let launch: Launch = serde_json::from_value(serde_json::json!({
        "id":"fixture","name":"Fixture","runtime_id":"fixture","cwd":"unused","model":"",
        "extra":[],"env":{"NO_PROXY":"existing"}, "token":"", "manual_router":false, "cols":80,"rows":24
    })).expect("launch fixture");
    let mut profile = profile(8000);
    assert!(adapter::binding(Agent::Claude, &launch, Some(&profile)).is_err());
    profile.anthropic_path = Some("".into());
    let old = adapter::binding(Agent::Claude, &launch, Some(&profile)).expect("old binding");
    profile.port = 8001;
    let new = adapter::binding(Agent::Claude, &launch, Some(&profile)).expect("new binding");
    assert_eq!(
        old.env.get("ANTHROPIC_BASE_URL").map(String::as_str),
        Some("http://127.0.0.1:8000")
    );
    assert_eq!(new.endpoint.as_deref(), Some("http://127.0.0.1:8001"));
    let no_proxy = &old.env["NO_PROXY"];
    assert!(no_proxy.split(',').any(|host| host == "existing"));
    assert!(no_proxy.split(',').any(|host| host == "127.0.0.1"));
}

#[test]
fn sqlite_migration_and_settings_survive_reopen() {
    let root = tempfile::tempdir().expect("temporary directory");
    let db = root.path().join("deck.db");
    let store = Store::open(&db).expect("migrate database");
    let prefs = Preferences {
        last_cwd: "한글 path".into(),
        ..Preferences::default()
    };
    store
        .import_settings(&prefs, &[profile(8001)])
        .expect("transactional import");
    drop(store);
    let reopened = Store::open(&db).expect("reopen database");
    assert_eq!(
        reopened
            .get::<Preferences>(Table::Settings, "terminal")
            .expect("read preferences")
            .expect("saved preferences")
            .last_cwd,
        prefs.last_cwd
    );
    assert_eq!(
        reopened
            .list::<Router>(Table::Routers)
            .expect("saved routers")[0]
            .port,
        8001
    );
}

#[test]
fn preset_migration_preserves_v1_database() {
    let root = tempfile::tempdir().expect("temporary directory");
    let db = root.path().join("old.db");
    let original = rusqlite::Connection::open(&db).unwrap();
    original
        .execute_batch(include_str!("../migrations/001_initial.sql"))
        .unwrap();
    let prefs = Preferences {
        last_cwd: "original workspace".into(),
        ..Preferences::default()
    };
    original
        .execute(
            "INSERT INTO application_settings (id, document) VALUES ('terminal', ?1)",
            [serde_json::to_string(&prefs).unwrap()],
        )
        .unwrap();
    drop(original);
    let store = Store::open(&db).unwrap();
    let restored = store
        .get::<Preferences>(Table::Settings, "terminal")
        .unwrap()
        .unwrap();
    assert_eq!(restored.last_cwd, prefs.last_cwd);
    assert!(store
        .list::<agent_deck::gjc::GjcPreset>(Table::GjcPresets)
        .unwrap()
        .is_empty());
}
