//! flvstx-ipc — WebSocket client to the agent sidecar, RPC bridging into `flvstx_core::ops`, and
//! sidecar process management. See `docs/protocol.md`.

pub mod chat;

use flvstx_core::ops::{describe, dispatch, Store};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::ErrorKind;
use std::net::TcpStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

pub const DEFAULT_PORT: u16 = 7878;

/// Events delivered to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Ready { backend: String, #[serde(default)] version: String },
    AssistantDelta { text: String },
    AssistantMessage { text: String },
    ToolCall { name: String, #[serde(default)] input: Value },
    ToolResult { name: String, #[serde(default)] summary: String },
    Rpc { id: u64, method: String, #[serde(default)] params: Value },
    Done { #[serde(default)] session_id: String, #[serde(default)] cost_usd: Option<f64>, #[serde(default)] turns: u32 },
    Error { message: String },
    Pong,
    /// Synthesised locally.
    #[serde(skip)]
    Connected,
    #[serde(skip)]
    Disconnected { reason: String },
    /// A session mutation happened while serving an RPC (UI should refresh).
    #[serde(skip)]
    SessionChanged,
}

enum Outgoing {
    Text(String),
    Close,
}

/// Background WebSocket client. Cheap to create; connects lazily in its own thread.
pub struct AgentClient {
    tx: Sender<Outgoing>,
    events: Receiver<AgentEvent>,
    connected: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    pub session_id: Arc<Mutex<Option<String>>>,
}

impl AgentClient {
    /// Connects to `ws://127.0.0.1:{port}`; RPCs are served against `store`.
    pub fn connect(port: u16, store: Arc<Mutex<Store>>) -> AgentClient {
        let (tx, rx) = channel::<Outgoing>();
        let (etx, events) = channel::<AgentEvent>();
        let connected = Arc::new(AtomicBool::new(false));
        let session_id = Arc::new(Mutex::new(None));
        let c2 = connected.clone();
        let sid = session_id.clone();
        let handle = std::thread::Builder::new()
            .name("flvstx-agent-ws".into())
            .spawn(move || run_client(port, store, rx, etx, c2, sid))
            .expect("spawn ws thread");
        AgentClient { tx, events, connected, handle: Some(handle), session_id }
    }

    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Sends a chat turn; `context` should be the compact session summary.
    pub fn send_user_message(&self, text: &str, context: &str) {
        let sid = self.session_id.lock().ok().and_then(|s| s.clone());
        let msg = json!({ "type": "user_message", "text": text, "context": context, "session_id": sid });
        let _ = self.tx.send(Outgoing::Text(msg.to_string()));
    }

    pub fn cancel(&self) {
        let _ = self.tx.send(Outgoing::Text(json!({ "type": "cancel" }).to_string()));
    }

    /// Non-blocking poll for UI events.
    pub fn try_recv(&self) -> Option<AgentEvent> {
        match self.events.try_recv() {
            Ok(e) => Some(e),
            Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => None,
        }
    }

    pub fn recv_timeout(&self, d: Duration) -> Option<AgentEvent> {
        self.events.recv_timeout(d).ok()
    }
}

impl Drop for AgentClient {
    fn drop(&mut self) {
        let _ = self.tx.send(Outgoing::Close);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn run_client(port: u16, store: Arc<Mutex<Store>>, rx: Receiver<Outgoing>, etx: Sender<AgentEvent>, connected: Arc<AtomicBool>, sid: Arc<Mutex<Option<String>>>) {
    let mut backoff = Duration::from_millis(300);
    loop {
        // Drain outgoing while disconnected so a Close still terminates us.
        while let Ok(o) = rx.try_recv() {
            if matches!(o, Outgoing::Close) {
                return;
            }
        }
        let url = format!("ws://127.0.0.1:{port}");
        let socket = match tungstenite::connect(&url) {
            Ok((s, _)) => s,
            Err(_) => {
                std::thread::sleep(backoff);
                backoff = (backoff * 2).min(Duration::from_secs(3));
                continue;
            }
        };
        backoff = Duration::from_millis(300);
        connected.store(true, Ordering::Relaxed);
        let _ = etx.send(AgentEvent::Connected);
        let reason = serve(socket, &store, &rx, &etx, &sid);
        connected.store(false, Ordering::Relaxed);
        let _ = etx.send(AgentEvent::Disconnected { reason: reason.clone() });
        if reason == "closed by client" {
            return;
        }
    }
}

fn serve(mut socket: WebSocket<tungstenite::stream::MaybeTlsStream<TcpStream>>, store: &Arc<Mutex<Store>>, rx: &Receiver<Outgoing>, etx: &Sender<AgentEvent>, sid: &Arc<Mutex<Option<String>>>) -> String {
    if let tungstenite::stream::MaybeTlsStream::Plain(s) = socket.get_ref() {
        let _ = s.set_nonblocking(true);
    }
    let mut last_ping = Instant::now();
    loop {
        // Outgoing.
        match rx.try_recv() {
            Ok(Outgoing::Text(t)) => {
                if socket.send(Message::Text(t.into())).is_err() {
                    return "send failed".into();
                }
            }
            Ok(Outgoing::Close) => {
                let _ = socket.close(None);
                return "closed by client".into();
            }
            Err(TryRecvError::Disconnected) => {
                let _ = socket.close(None);
                return "closed by client".into();
            }
            Err(TryRecvError::Empty) => {}
        }
        if last_ping.elapsed() > Duration::from_secs(15) {
            let _ = socket.send(Message::Ping(vec![].into()));
            last_ping = Instant::now();
        }
        // Incoming.
        match socket.read() {
            Ok(Message::Text(t)) => {
                let text = t.as_str().to_string();
                match serde_json::from_str::<AgentEvent>(&text) {
                    Ok(AgentEvent::Rpc { id, method, params }) => {
                        let (ok, payload) = {
                            let mut guard = match store.lock() {
                                Ok(g) => g,
                                Err(p) => p.into_inner(),
                            };
                            match dispatch(&mut guard, &method, &params) {
                                Ok(v) => (true, v),
                                Err(e) => (false, Value::String(e.to_string())),
                            }
                        };
                        let _ = etx.send(AgentEvent::Rpc { id, method: method.clone(), params });
                        if ok && method != "get_session" && method != "get_notes" && method != "analyze" && method != "suggest_chords" && method != "list_scales" {
                            let _ = etx.send(AgentEvent::SessionChanged);
                        }
                        let reply = if ok { json!({ "type": "rpc_result", "id": id, "ok": true, "result": payload }) } else { json!({ "type": "rpc_result", "id": id, "ok": false, "error": payload }) };
                        if socket.send(Message::Text(reply.to_string().into())).is_err() {
                            return "send failed".into();
                        }
                    }
                    Ok(ev) => {
                        if let AgentEvent::Done { session_id, .. } = &ev {
                            if !session_id.is_empty() {
                                if let Ok(mut g) = sid.lock() {
                                    *g = Some(session_id.clone());
                                }
                            }
                        }
                        let _ = etx.send(ev);
                    }
                    Err(e) => {
                        let _ = etx.send(AgentEvent::Error { message: format!("bad frame from agent: {e}: {}", text.chars().take(200).collect::<String>()) });
                    }
                }
            }
            Ok(Message::Close(_)) => return "agent closed".into(),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e)) if e.kind() == ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return format!("read failed: {e}"),
        }
    }
}

/// Builds the context string sent with each user message.
pub fn context_for(store: &Store) -> String {
    describe(&store.session)
}

/// Locates the agent directory: `FLVSTX_AGENT_DIR`, else `<exe dir>/../../agent`, else `D:\FLVSTX\agent`.
pub fn agent_dir() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("FLVSTX_AGENT_DIR") {
        return d.into();
    }
    if let Ok(exe) = std::env::current_exe() {
        for anc in exe.ancestors().take(5) {
            let cand = anc.join("agent");
            if cand.join("package.json").exists() {
                return cand;
            }
        }
    }
    std::path::PathBuf::from(r"D:\FLVSTX\agent")
}

/// Spawns the sidecar (`node dist/index.js --port N`), returning the child process.
pub fn spawn_agent(port: u16) -> std::io::Result<std::process::Child> {
    let dir = agent_dir();
    let entry = if dir.join("dist").join("index.js").exists() { dir.join("dist").join("index.js") } else { dir.join("src").join("index.ts") };
    let mut cmd = if entry.extension().map(|e| e == "ts").unwrap_or(false) {
        let mut c = std::process::Command::new("npx");
        c.arg("tsx").arg(&entry);
        c
    } else {
        let mut c = std::process::Command::new("node");
        c.arg(&entry);
        c
    };
    cmd.arg("--port").arg(port.to_string()).current_dir(&dir).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::inherit()).stderr(std::process::Stdio::inherit());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd.spawn()
}

/// True if something is already listening on the port.
pub fn port_open(port: u16) -> bool {
    TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(200)).is_ok()
}
