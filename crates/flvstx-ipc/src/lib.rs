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

/// One thing the producer intends to do. `targets` is what it will write, as `layer@section`,
/// so approving a plan is approving a list of changes rather than a paragraph of prose.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanStep {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub detail: String,
    #[serde(default)]
    pub targets: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlanSection {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub bars: u32,
    #[serde(default)]
    pub role: Option<String>,
}

/// What the producer proposes before it is allowed to change anything.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Plan {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub key: Option<String>,
    #[serde(default)]
    pub tempo: Option<f32>,
    #[serde(default)]
    pub time_signature: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub form: Vec<PlanSection>,
    #[serde(default)]
    pub layers: Vec<String>,
    #[serde(default)]
    pub steps: Vec<PlanStep>,
    #[serde(default)]
    pub revision: u32,
    #[serde(default)]
    pub risks: Vec<String>,
}

/// Events delivered to the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    Ready { backend: String, #[serde(default)] version: String, #[serde(default)] modes: Vec<String> },
    AssistantDelta { text: String, #[serde(default)] agent: Option<String> },
    AssistantMessage { text: String, #[serde(default)] agent: Option<String> },
    ToolCall {
        name: String,
        #[serde(default)]
        input: Value,
        #[serde(default)]
        tool_use_id: Option<String>,
        #[serde(default)]
        agent: Option<String>,
    },
    ToolResult {
        name: String,
        #[serde(default)]
        summary: String,
        #[serde(default)]
        tool_use_id: Option<String>,
        #[serde(default)]
        agent: Option<String>,
        #[serde(default)]
        ok: Option<bool>,
    },
    Rpc { id: u64, method: String, #[serde(default)] params: Value },
    Done { #[serde(default)] session_id: String, #[serde(default)] cost_usd: Option<f64>, #[serde(default)] turns: u32, #[serde(default)] mode: Option<String> },
    Error { message: String, #[serde(default)] code: Option<String>, #[serde(default)] agent: Option<String> },
    Pong,
    /// The producer wants approval before it writes anything.
    PlanProposed { plan_id: String, #[serde(default)] plan: Plan },
    /// The answer landed (or was too late — `decision: "stale"`).
    PlanResolved { plan_id: String, decision: String, #[serde(default)] notes: Option<String> },
    /// Where a producer turn is: idle, planning, awaiting_approval, executing.
    Phase { phase: String },
    /// Synthesised locally.
    #[serde(skip)]
    Connected,
    #[serde(skip)]
    Disconnected { reason: String },
    /// A session mutation happened while serving an RPC (UI should refresh).
    #[serde(skip)]
    SessionChanged,
    /// A frame this build does not understand. The sidecar and the plugin ship separately and the
    /// sidecar gains events first, so an unknown tag has to be ignorable — before this existed,
    /// every such frame became a red line in the user's transcript. Must stay last: serde requires
    /// the catch-all to be the final variant.
    #[serde(other)]
    Unknown,
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
        self.send_user_message_with_model(text, context, None);
    }

    /// Sends a chat turn, optionally switching the model ("sonnet", "opus", or a full model id).
    pub fn send_user_message_with_model(&self, text: &str, context: &str, model: Option<&str>) {
        self.send_user_message_in(text, context, model, "composer");
    }

    /// `mode` picks the persona and tool set on the sidecar: "composer" answers directly,
    /// "producer" plans first and waits for approval.
    pub fn send_user_message_in(&self, text: &str, context: &str, model: Option<&str>, mode: &str) {
        let sid = self.session_id.lock().ok().and_then(|s| s.clone());
        let msg = json!({ "type": "user_message", "text": text, "context": context, "session_id": sid, "model": model, "mode": mode });
        let _ = self.tx.send(Outgoing::Text(msg.to_string()));
    }

    /// Answers a proposed plan. Not a user message: the producer is parked inside a tool call, so
    /// the sidecar's one-turn gate would reject it and queueing it would deadlock.
    pub fn send_plan_decision(&self, plan_id: &str, decision: &str, notes: Option<&str>) {
        let msg = serde_json::json!({ "type": "plan_decision", "plan_id": plan_id, "decision": decision, "notes": notes });
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
    // Messages sent before the socket is up are queued and flushed after connecting.
    let mut pending: Vec<String> = Vec::new();
    loop {
        while let Ok(o) = rx.try_recv() {
            match o {
                Outgoing::Close => return,
                Outgoing::Text(t) => pending.push(t),
            }
        }
        let url = format!("ws://127.0.0.1:{port}");
        let mut socket = match tungstenite::connect(&url) {
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
        for t in pending.drain(..) {
            let _ = socket.send(Message::Text(t.into()));
        }
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
                        if matches!(ev, AgentEvent::Unknown) {
                            log_frame(&format!("ignoring unknown agent frame: {}", text.chars().take(160).collect::<String>()));
                        } else {
                            let _ = etx.send(ev);
                        }
                    }
                    Err(e) => {
                        // A known tag with a shape this build cannot read. Worth a log line, not a
                        // wall of red in the transcript.
                        let tag = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["type"].as_str().map(str::to_owned)).unwrap_or_default();
                        log_frame(&format!("agent frame '{tag}' did not parse: {e}"));
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
/// Diagnostics for frames the UI cannot use. Goes to the agent log rather than the transcript.
fn log_frame(msg: &str) {
    let dir = flvstx_core::midi::default_export_dir();
    let path = dir.parent().unwrap_or(&dir).join("agent.log");
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        use std::io::Write;
        let _ = writeln!(f, "[plugin] {msg}");
    }
}

pub fn context_for(store: &Store) -> String {
    describe(&store.session)
}

/// Locates the composer sidecar: `FLVSTX_AGENT_DIR`, else an `agent` folder beside the
/// executable (a repo checkout), else the installed copy under `%LOCALAPPDATA%/FLVSTX/agent`.
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
    flvstx_core::midi::default_export_dir().parent().map(|p| p.join("agent")).unwrap_or_else(|| std::path::PathBuf::from("agent"))
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
    // GUI hosts (FL Studio) have no console; inherited stdio handles would be invalid, so log to a file.
    let log_dir = flvstx_core::midi::default_export_dir().parent().map(|p| p.to_path_buf()).unwrap_or_else(std::env::temp_dir);
    let _ = std::fs::create_dir_all(&log_dir);
    let log = std::fs::OpenOptions::new().create(true).append(true).open(log_dir.join("agent.log")).ok();
    let (out, err) = match log {
        Some(f) => (std::process::Stdio::from(f.try_clone().unwrap_or(f)), std::process::Stdio::null()),
        None => (std::process::Stdio::null(), std::process::Stdio::null()),
    };
    let err = match std::fs::OpenOptions::new().create(true).append(true).open(log_dir.join("agent.log")) {
        Ok(f) => std::process::Stdio::from(f),
        Err(_) => err,
    };
    cmd.arg("--port").arg(port.to_string()).current_dir(&dir).stdin(std::process::Stdio::null()).stdout(out).stderr(err);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The sidecar and the plugin ship separately and the sidecar learns new frames first, so an
    /// unknown event must be ignorable rather than an error in the user's transcript.
    #[test]
    fn unknown_frames_are_tolerated() {
        let ev: AgentEvent = serde_json::from_str(r#"{"type":"subagent_start","agent":"harmony-form"}"#).expect("unknown tag must parse");
        assert!(matches!(ev, AgentEvent::Unknown));
    }

    /// A plan with only the fields the producer bothered to fill in still loads, and an empty one
    /// does not panic the card that renders it.
    #[test]
    fn plans_parse_from_a_partial_frame() {
        let ev: AgentEvent = serde_json::from_str(r#"{"type":"plan_proposed","plan_id":"p1","plan":{}}"#).unwrap();
        match ev {
            AgentEvent::PlanProposed { plan_id, plan } => {
                assert_eq!(plan_id, "p1");
                assert!(plan.steps.is_empty() && plan.summary.is_empty() && plan.tempo.is_none());
            }
            other => panic!("expected a proposed plan, got {other:?}"),
        }
        let ev: AgentEvent = serde_json::from_str(
            r#"{"type":"plan_proposed","plan_id":"p2","plan":{"summary":"lofi","tempo":82,"revision":2,"form":[{"name":"Verse","bars":8}],"steps":[{"id":"s1","owner":"rhythm-section","title":"Drums","detail":"","targets":["drums@verse"]}]}}"#,
        )
        .unwrap();
        match ev {
            AgentEvent::PlanProposed { plan, .. } => {
                assert_eq!(plan.revision, 2);
                assert_eq!(plan.tempo, Some(82.0));
                assert_eq!(plan.form[0].bars, 8);
                assert_eq!(plan.steps[0].targets, vec!["drums@verse".to_string()]);
                assert!(plan.risks.is_empty());
            }
            other => panic!("expected a proposed plan, got {other:?}"),
        }
    }

    /// ...and a known frame that gains fields must still load on an older build.
    #[test]
    fn new_fields_default() {
        let ev: AgentEvent = serde_json::from_str(r#"{"type":"tool_call","name":"generate","input":{}}"#).unwrap();
        match ev {
            AgentEvent::ToolCall { name, tool_use_id, agent, .. } => {
                assert_eq!(name, "generate");
                assert!(tool_use_id.is_none() && agent.is_none());
            }
            other => panic!("expected a tool call, got {other:?}"),
        }
        let ev: AgentEvent = serde_json::from_str(r#"{"type":"tool_call","name":"generate","input":{},"tool_use_id":"tu_1","agent":"rhythm-section"}"#).unwrap();
        match ev {
            AgentEvent::ToolCall { tool_use_id, agent, .. } => {
                assert_eq!(tool_use_id.as_deref(), Some("tu_1"));
                assert_eq!(agent.as_deref(), Some("rhythm-section"));
            }
            other => panic!("expected a tool call, got {other:?}"),
        }
    }
}
