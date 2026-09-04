//! Terminal chat harness: the same RPC bridge the plugin uses, driven from stdin.

use crate::{context_for, port_open, spawn_agent, AgentClient, AgentEvent};
use flvstx_core::ops::Store;
use flvstx_core::Session;
use std::io::{BufRead, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn run_terminal_chat(session: Session, port: u16, save_path: Option<String>) -> anyhow::Result<()> {
    let store = Arc::new(Mutex::new(Store::new(session)));
    let mut child = None;
    if !port_open(port) {
        eprintln!("[chat] starting agent sidecar on port {port}…");
        child = Some(spawn_agent(port)?);
    }
    let client = AgentClient::connect(port, store.clone());
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    while !client.is_connected() {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("agent did not come up on port {port}");
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    eprintln!("[chat] connected. Type a message, or /session, /export, /undo, /save, /quit.");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    loop {
        print!("\nyou> ");
        out.flush()?;
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match line {
            "/quit" | "/exit" => break,
            "/session" => {
                println!("{}", context_for(&store.lock().unwrap()));
                continue;
            }
            "/undo" => {
                let ok = store.lock().unwrap().undo();
                println!("undo: {ok}");
                continue;
            }
            "/export" => {
                let mut g = store.lock().unwrap();
                match flvstx_core::ops::dispatch(&mut g, "export", &serde_json::json!({})) {
                    Ok(v) => println!("{}", serde_json::to_string_pretty(&v)?),
                    Err(e) => println!("error: {e}"),
                }
                continue;
            }
            "/save" => {
                if let Some(p) = &save_path {
                    std::fs::write(p, serde_json::to_string_pretty(&store.lock().unwrap().session)?)?;
                    println!("saved {p}");
                } else {
                    println!("no session path given");
                }
                continue;
            }
            _ => {}
        }
        let ctx = context_for(&store.lock().unwrap());
        client.send_user_message(line, &ctx);
        // Stream events until done.
        let mut streaming_line = false;
        loop {
            match client.recv_timeout(Duration::from_secs(600)) {
                Some(AgentEvent::AssistantDelta { text }) => {
                    print!("{text}");
                    out.flush()?;
                    streaming_line = true;
                }
                Some(AgentEvent::AssistantMessage { text }) => {
                    if !streaming_line {
                        println!("\nclaude> {text}");
                    } else {
                        println!();
                        streaming_line = false;
                    }
                }
                Some(AgentEvent::ToolCall { name, input }) => {
                    if streaming_line {
                        println!();
                        streaming_line = false;
                    }
                    let short = serde_json::to_string(&input).unwrap_or_default();
                    println!("  [tool] {name} {}", short.chars().take(160).collect::<String>());
                }
                Some(AgentEvent::ToolResult { name, summary }) => {
                    println!("  [result] {name}: {}", summary.lines().take(3).collect::<Vec<_>>().join(" | ").chars().take(200).collect::<String>());
                }
                Some(AgentEvent::Done { turns, cost_usd, .. }) => {
                    if streaming_line {
                        println!();
                    }
                    println!("  [done] turns={turns} cost={}", cost_usd.map(|c| format!("${c:.3}")).unwrap_or_else(|| "-".into()));
                    break;
                }
                Some(AgentEvent::Error { message }) => {
                    println!("\n  [error] {message}");
                    break;
                }
                Some(AgentEvent::Disconnected { reason }) => {
                    println!("\n  [disconnected] {reason}");
                    break;
                }
                Some(_) => {}
                None => {
                    println!("\n  [timeout]");
                    break;
                }
            }
        }
        if let Some(p) = &save_path {
            let _ = std::fs::write(p, serde_json::to_string_pretty(&store.lock().unwrap().session)?);
        }
    }
    drop(client);
    if let Some(mut c) = child {
        let _ = c.kill();
    }
    Ok(())
}
