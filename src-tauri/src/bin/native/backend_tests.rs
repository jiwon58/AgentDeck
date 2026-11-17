use super::*;
use agent_deck::{native_gjc::HistoryLocation, terminal::Payload};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

struct MockRouter {
    exit: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    port: u16,
}

impl MockRouter {
    fn start() -> Self {
        let server = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = server.local_addr().unwrap().port();
        server.set_nonblocking(true).unwrap();
        let exit = Arc::new(AtomicBool::new(false));
        let stop = exit.clone();
        let thread = std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let Ok((mut stream, _)) = server.accept() else {
                    std::thread::sleep(Duration::from_millis(10));
                    continue;
                };
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut reader = BufReader::new(&stream);
                let mut path = String::new();
                if reader.read_line(&mut path).is_err() {
                    continue;
                }
                let mut length = 0;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                        break;
                    }
                    if let Some(size) = header.to_ascii_lowercase().strip_prefix("content-length:")
                    {
                        length = size.trim().parse::<usize>().unwrap_or(0).min(128 * 1024);
                    }
                }
                let mut body = vec![0; length];
                if reader.read_exact(&mut body).is_err() {
                    continue;
                }
                let (kind, response) = if path.starts_with("GET ") {
                    ("application/json", r#"{"data":[{"id":"alpha","max_model_len":65536},{"id":"beta","max_model_len":32768}]}"#.to_owned())
                } else {
                    let chunk = |text: &str| format!("data: {text}\n\n");
                    ("text/event-stream", [
                        chunk(r#"{"id":"native","object":"chat.completion.chunk","created":0,"model":"alpha","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}"#),
                        chunk(r#"{"id":"native","object":"chat.completion.chunk","created":0,"model":"alpha","choices":[{"index":0,"delta":{"content":"Native Router fixture reply"},"finish_reason":null}]}"#),
                        chunk(r#"{"id":"native","object":"chat.completion.chunk","created":0,"model":"alpha","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#),
                        chunk("[DONE]"),
                    ].concat())
                };
                let _ = write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len());
            }
        });
        Self {
            port,
            exit,
            thread: Some(thread),
        }
    }
}

impl Drop for MockRouter {
    fn drop(&mut self) {
        self.exit.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn read_until(term: &Terminal, id: &str, text: &str) -> String {
    let deadline = Instant::now() + Duration::from_secs(24);
    let mut output = String::new();
    let mut answered = 0;
    while Instant::now() < deadline {
        for event in term.drain(id).unwrap() {
            if let Payload::Bytes(bytes) = event.payload {
                output.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        while answered < output.matches("\x1b[6n").count() {
            term.write(id, "\x1b[1;1R".into()).unwrap();
            answered += 1;
        }
        if output.contains(text) {
            return output;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let last: String = output
        .chars()
        .rev()
        .take(500)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    panic!("Timed out looking for {text}; tail {last:?}");
}

#[test]
fn real_gjc_consumes_the_generated_yaml_and_writes_to_the_selected_history() {
    let Some(binary) = std::env::var_os("DECK_GJC_0102") else {
        eprintln!("Set DECK_GJC_0102 to run the real GJC native integration");
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let root = dunce::canonicalize(temp.path()).unwrap();
    let cwd = root.join("한글 native workspace");
    std::fs::create_dir(&cwd).unwrap();
    let backend = Backend::open(root.join("app")).unwrap();
    let server = MockRouter::start();
    let candidate = registry::inspect(binary.to_str().unwrap()).unwrap();
    let runtime = backend
        .register(&candidate.path, &candidate.sha256, Agent::Gajae)
        .unwrap();
    assert_eq!(runtime.version, "0.10.2");
    let profile = Router {
        id: "local-fixture".into(),
        name: "Local fixture".into(),
        scheme: "http".into(),
        host: "127.0.0.1".into(),
        port: server.port,
        openai_path: Some("/v1".into()),
        anthropic_path: None,
        model: "".into(),
        authentication: "none".into(),
        enabled: true,
        allow_remote: false,
        created_at: 0,
        updated_at: 0,
    };
    backend
        .store
        .put(Table::Routers, &profile.id, &profile)
        .unwrap();
    let config = root.join("baseline-config.yml");
    std::fs::write(&config, "tools:\n  discoveryMode: off\n").unwrap();
    let preset = Preset {
        router_id: profile.id.clone(),
        default_model: "alpha".into(),
        context: 8192,
        max_output: 512,
        config_template: config.display().to_string(),
        location: HistoryLocation::Workspace,
        ..Preset::default()
    };
    let session = backend
        .launch(
            "Native integration".into(),
            &runtime.id,
            Mode::Preset {
                cwd: cwd.display().to_string(),
                preset: Box::new(preset),
                token: String::new(),
                catalog: vec!["alpha".into(), "beta".into()],
            },
            || {},
        )
        .unwrap();
    let paths = session.native_gjc.as_ref().unwrap();
    let models = std::fs::read_to_string(paths.agent_directory.join("models.yml")).unwrap();
    assert!(models.contains("65536") && models.contains("32768"));
    read_until(&backend.terminal, &session.id, "alpha");
    backend
        .terminal
        .write(&session.id, "Native integration prompt\r".into())
        .unwrap();
    let mut output = read_until(
        &backend.terminal,
        &session.id,
        "Native Router fixture reply",
    );
    let persisted = Instant::now() + Duration::from_secs(10);
    let entry = loop {
        let entries = gjc_history::list_in(
            paths.session_directory.clone(),
            paths.agent_directory.clone(),
        )
        .unwrap()
        .entries;
        if let Some(entry) = entries.into_iter().find(|entry| {
            std::fs::read_to_string(&entry.source.path)
                .is_ok_and(|contents| contents.contains("Native integration prompt"))
        }) {
            break entry;
        }
        assert!(Instant::now() < persisted,
            "GJC did not save the native conversation; expected entries {:?}; agent entries {:?}; tail {:?}",
            std::fs::read_dir(&paths.session_directory).unwrap().flatten().map(|entry| entry.file_name()).collect::<Vec<_>>(),
            std::fs::read_dir(&paths.agent_directory).unwrap().flatten().map(|entry| entry.file_name()).collect::<Vec<_>>(),
            output.chars().rev().take(2300).collect::<Vec<_>>().into_iter().rev().collect::<String>());
        for event in backend.terminal.drain(&session.id).unwrap() {
            if let Payload::Bytes(bytes) = event.payload {
                output.push_str(&String::from_utf8_lossy(&bytes));
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    backend.terminal.close(&session.id).unwrap();
    let resumed = backend
        .launch(
            "Native integration".into(),
            &runtime.id,
            Mode::Resume {
                source: entry.source.clone(),
                model: String::new(),
                token: String::new(),
            },
            || {},
        )
        .unwrap();
    assert_eq!(
        resumed.resume_source_id.as_deref(),
        Some(session.id.as_str())
    );
    backend.terminal.close(&resumed.id).unwrap();
    assert_eq!(
        backend
            .store
            .list::<Session>(Table::Sessions)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        std::fs::read_to_string(config).unwrap(),
        "tools:\n  discoveryMode: off\n"
    );
    if let Some(gjc) = std::env::var_os("DECK_GJC_0200") {
        let newer = root.join("gjc-0200");
        std::fs::create_dir(&newer).unwrap();
        for name in ["models.yml", "config.yml"] {
            std::fs::copy(paths.agent_directory.join(name), newer.join(name)).unwrap();
        }
        let list = std::process::Command::new(gjc)
            .current_dir(&cwd)
            .env("GJC_CODING_AGENT_DIR", &newer)
            .env("GJC_MODEL_PRESET_REGISTRY_DISABLED", "1")
            .args(["--list-models", "deck-router"])
            .output()
            .unwrap();
        assert!(list.status.success());
        let listing = String::from_utf8_lossy(&list.stdout);
        assert!(
            listing.contains("alpha") && listing.contains("beta"),
            "{listing}"
        );
    }
}
