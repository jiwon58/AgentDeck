use crate::{
    diagnostics,
    model::{error, Result},
    process::ProcessTree,
};
use portable_pty::{native_pty_system, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use std::{
    collections::{HashMap, VecDeque},
    io::{Read, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

const QUEUE_LIMIT: usize = 28;
const SESSION_LIMIT: usize = 24;

#[derive(Serialize)]
pub struct Event {
    pub session_id: String,
    pub event_type: &'static str,
    pub sequence_number: u64,
    pub payload: serde_json::Value,
}

struct Output {
    queue: VecDeque<Event>,
    next: u64,
    reported_overflow: bool,
}

struct Live {
    master: Mutex<Option<Box<dyn MasterPty + Send>>>,
    input: mpsc::SyncSender<Vec<u8>>,
    stop: AtomicBool,
    done: AtomicBool,
    state: Mutex<&'static str>,
    output: Mutex<Output>,
    notify: Arc<dyn Fn(&Event) + Send + Sync>,
    id: String,
}

impl Live {
    fn push(&self, kind: &'static str, payload: serde_json::Value) {
        if let Ok(mut output) = self.output.lock() {
            let notify = output.queue.is_empty();
            let sequence_number = output.next;
            output.next += 1;
            if output.queue.len() == QUEUE_LIMIT {
                output.queue.pop_front();
                if !output.reported_overflow {
                    output.reported_overflow = true;
                    diagnostics::warn("terminal.output_queue_overflow", Some(&self.id));
                }
            }
            output.queue.push_back(Event {
                session_id: self.id.clone(),
                event_type: kind,
                sequence_number,
                payload,
            });
            if notify {
                (self.notify)(&Event {
                    session_id: self.id.clone(),
                    event_type: kind,
                    sequence_number,
                    payload: serde_json::Value::Null,
                });
            }
        }
    }

    fn state(&self, state: &'static str) {
        if let Ok(mut value) = self.state.lock() {
            *value = state;
        }
    }
}

#[derive(Default)]
pub struct Terminal {
    sessions: Mutex<HashMap<String, Arc<Live>>>,
    closing: AtomicBool,
}

fn size(cols: u16, rows: u16) -> PtySize {
    PtySize {
        cols: cols.clamp(2, 1000),
        rows: rows.clamp(2, 1000),
        pixel_width: 0,
        pixel_height: 0,
    }
}

impl Terminal {
    pub fn start(
        &self,
        id: String,
        command: CommandBuilder,
        dimensions: (u16, u16),
        notify: impl Fn(&Event) + Send + Sync + 'static,
    ) -> Result<()> {
        let mut sessions = self.sessions.lock().map_err(error)?;
        if self.closing.load(Ordering::Acquire) {
            return Err("Application is closing".into());
        }
        if sessions.contains_key(&id) {
            return Err("Session ID is already in use".into());
        }
        if sessions.len() >= SESSION_LIMIT {
            return Err("Close a tab before launching more than 24 sessions".into());
        }
        let pair = diagnostics::stage(
            native_pty_system().openpty(size(dimensions.0, dimensions.1)),
            "terminal.open_pty_failed",
            Some(&id),
        )
        .map_err(error)?;
        let mut reader = pair.master.try_clone_reader().map_err(error)?;
        let mut writer = pair.master.take_writer().map_err(error)?;
        let job = diagnostics::stage(ProcessTree::new(), "terminal.create_job_failed", Some(&id))?;
        let mut child = diagnostics::stage(
            pair.slave.spawn_command(command),
            "terminal.spawn_failed",
            Some(&id),
        )
        .map_err(error)?;
        drop(pair.slave);
        #[cfg(windows)]
        {
            let attached = child
                .as_raw_handle()
                .ok_or_else(|| "PTY process handle unavailable".to_owned())
                .and_then(|handle| job.attach(handle as _));
            if let Err(e) = attached {
                diagnostics::warn("terminal.contain_process_failed", Some(&id));
                let _ = child.kill();
                let _ = child.wait();
                return Err(e);
            }
        }
        let (input, receive) = mpsc::sync_channel(32);
        let live = Arc::new(Live {
            master: Mutex::new(Some(pair.master)),
            input,
            stop: AtomicBool::new(false),
            done: AtomicBool::new(false),
            state: Mutex::new("running"),
            output: Mutex::new(Output {
                queue: VecDeque::new(),
                next: 0,
                reported_overflow: false,
            }),
            notify: Arc::new(notify),
            id: id.clone(),
        });
        sessions.insert(id, Arc::clone(&live));
        diagnostics::info("terminal.started", Some(&live.id));
        live.push("terminal_started", serde_json::Value::Null);

        let output = Arc::clone(&live);
        let reading = thread::spawn(move || {
            let mut buffer = [0; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        output.push("terminal_output", serde_json::json!(&buffer[..count]))
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    // Windows reports a broken pipe when the pseudoconsole closes.
                    Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => break,
                    Err(e) => {
                        if !output.stop.load(Ordering::Acquire) {
                            diagnostics::record(
                                "WARN",
                                "terminal.output_pipe_failed",
                                Some(&output.id),
                                e.raw_os_error().map(i64::from),
                                None,
                            );
                            output.push("terminal_error", serde_json::json!(e.to_string()));
                        }
                        break;
                    }
                }
            }
        });
        let input = Arc::clone(&live);
        thread::spawn(move || {
            while !input.done.load(Ordering::Acquire) {
                match receive.recv_timeout(Duration::from_millis(100)) {
                    Ok(bytes)
                        if writer
                            .write_all(&bytes)
                            .and_then(|_| writer.flush())
                            .is_err() =>
                    {
                        if !input.stop.load(Ordering::Acquire) {
                            diagnostics::warn("terminal.input_pipe_failed", Some(&input.id));
                            input.push(
                                "terminal_error",
                                serde_json::json!("Terminal input pipe closed"),
                            );
                        }
                        break;
                    }
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        thread::spawn(move || {
            let mut deadline = None;
            let result = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break Ok(status),
                    Ok(None) => {}
                    Err(e) => {
                        let _ = child.kill();
                        break Err(e);
                    }
                }
                if live.stop.load(Ordering::Acquire) && deadline.is_none() {
                    live.state("stopping");
                    let _ = live.input.try_send(vec![3]);
                    deadline = Some(Instant::now() + Duration::from_millis(850));
                }
                if deadline.is_some_and(|time| Instant::now() >= time) {
                    diagnostics::info("terminal.force_stop", Some(&live.id));
                    let _ = child.kill();
                    break child.wait();
                }
                thread::sleep(Duration::from_millis(30));
            };
            drop(job);
            live.done.store(true, Ordering::Release);
            let master = live.master.lock().ok().and_then(|mut master| master.take());
            drop(master);
            let _ = reading.join();
            let (state, payload) = match result {
                Ok(status) => {
                    let state = if live.stop.load(Ordering::Acquire) {
                        "stopped"
                    } else {
                        "exited"
                    };
                    diagnostics::record(
                        if state == "exited" && !status.success() {
                            "WARN"
                        } else {
                            "INFO"
                        },
                        "terminal.exited",
                        Some(&live.id),
                        Some(i64::from(status.exit_code())),
                        Some(state),
                    );
                    (
                        state,
                        serde_json::json!({"status": state, "code": status.exit_code()}),
                    )
                }
                Err(e) => {
                    diagnostics::warn("terminal.process_wait_failed", Some(&live.id));
                    live.push("terminal_error", serde_json::json!(e.to_string()));
                    (
                        "failed",
                        serde_json::json!({"status": "failed", "code": null}),
                    )
                }
            };
            live.state(state);
            live.push("terminal_exited", payload);
        });
        Ok(())
    }

    fn live(&self, id: &str) -> Result<Arc<Live>> {
        self.sessions
            .lock()
            .map_err(error)?
            .get(id)
            .cloned()
            .ok_or_else(|| "Terminal tab is unavailable".into())
    }

    pub fn write(&self, id: &str, data: String) -> Result<()> {
        if data.len() > 64 * 1024 {
            return Err("Terminal input chunk exceeds 64 KiB".into());
        }
        self.live(id)?
            .input
            .try_send(data.into_bytes())
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => "Terminal input is busy".into(),
                mpsc::TrySendError::Disconnected(_) => "Terminal input is closed".into(),
            })
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<()> {
        let live = self.live(id)?;
        let master = live.master.lock().map_err(error)?;
        if let Some(master) = master.as_ref() {
            master.resize(size(cols, rows)).map_err(error)?;
            live.push(
                "terminal_resized",
                serde_json::json!({ "cols": cols, "rows": rows }),
            );
        }
        Ok(())
    }

    pub fn drain(&self, id: &str) -> Result<Vec<Event>> {
        let live = self.live(id)?;
        let mut output = live.output.lock().map_err(error)?;
        Ok(output.queue.drain(..).collect())
    }

    pub fn status(&self, id: &str) -> Result<&'static str> {
        let Ok(live) = self.live(id) else {
            return Ok("saved");
        };
        let status = *live.state.lock().map_err(error)?;
        Ok(status)
    }

    pub fn close(&self, id: &str) -> Result<()> {
        if let Ok(live) = self.live(id) {
            live.stop.store(true, Ordering::Release);
            let deadline = Instant::now() + Duration::from_secs(4);
            while !live.done.load(Ordering::Acquire) && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(25));
            }
            if !live.done.load(Ordering::Acquire) {
                return Err("Terminal is still stopping. Try again.".into());
            }
            self.sessions.lock().map_err(error)?.remove(id);
        }
        Ok(())
    }

    pub fn active_count(&self) -> Result<usize> {
        Ok(self
            .sessions
            .lock()
            .map_err(error)?
            .values()
            .filter(|session| !session.done.load(Ordering::Acquire))
            .count())
    }

    pub fn shutdown(&self) -> Result<()> {
        self.closing.store(true, Ordering::Release);
        let mut sessions = self.sessions.lock().map_err(error)?;
        for session in sessions.values() {
            session.stop.store(true, Ordering::Release);
        }
        let deadline = Instant::now() + Duration::from_secs(4);
        while sessions
            .values()
            .any(|session| !session.done.load(Ordering::Acquire))
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(25));
        }
        if sessions
            .values()
            .any(|session| !session.done.load(Ordering::Acquire))
        {
            self.closing.store(false, Ordering::Release);
            return Err("Some sessions are still stopping. Try closing again.".into());
        }
        sessions.clear();
        Ok(())
    }
}
