//! Terminal chat harness: the same RPC bridge the plugin uses, driven from stdin.

use crate::{context_for, port_open, spawn_agent, AgentClient, AgentEvent, Backend, Plan, DEFAULT_OLLAMA_URL};
use tonefold_core::ops::Store;
use tonefold_core::Session;
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
    eprintln!("[chat] /producer plans before it writes and asks you to approve; /composer answers straight away.");
    eprintln!("[chat] /ollama [URL] runs on an Ollama server (then /models to list, /model NAME to pick); /claude switches back.");
    eprintln!("[chat] /think on|off: whether the model thinks before answering (Claude: on, Ollama: off unless asked).");
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    // Which persona the sidecar runs. Switching ends one SDK session and resumes the other.
    let mut mode = String::from("composer");
    let mut model: Option<String> = None;
    let mut provider = String::from("claude");
    let mut ollama_url = String::from(DEFAULT_OLLAMA_URL);
    let mut think: Option<bool> = None;
    loop {
        print!("\n{mode}> ");
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
            "/producer" | "/composer" => {
                mode = line.trim_start_matches('/').to_string();
                println!("mode: {mode}");
                continue;
            }
            "/undo" => {
                let ok = store.lock().unwrap().undo();
                println!("undo: {ok}");
                continue;
            }
            "/export" => {
                let mut g = store.lock().unwrap();
                match tonefold_core::ops::dispatch(&mut g, "export", &serde_json::json!({})) {
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
        if let Some(rest) = line.strip_prefix("/model ") {
            model = match rest.trim() {
                "" | "default" => None,
                m => Some(m.to_string()),
            };
            println!("model: {}", model.as_deref().unwrap_or("default"));
            continue;
        }
        if line == "/claude" {
            provider = "claude".into();
            model = None;
            println!("provider: claude (model: default)");
            continue;
        }
        if line == "/ollama" || line.starts_with("/ollama ") {
            provider = "ollama".into();
            if let Some(url) = line.strip_prefix("/ollama").map(str::trim).filter(|u| !u.is_empty()) {
                ollama_url = url.to_string();
            }
            println!("provider: ollama at {ollama_url} — /models lists what it can run, /model NAME picks one");
            continue;
        }
        if let Some(rest) = line.strip_prefix("/think") {
            think = Some(matches!(rest.trim(), "on" | "1" | "true" | "yes"));
            println!("think: {}", if think == Some(true) { "on" } else { "off" });
            continue;
        }
        if line == "/models" {
            client.list_models(&ollama_url);
            match wait_for_models(&client) {
                Some(AgentEvent::Models { models, error: None, base_url, .. }) => {
                    println!("{base_url}: {}", if models.is_empty() { "no models pulled yet".to_string() } else { models.join(", ") });
                }
                Some(AgentEvent::Models { error: Some(e), .. }) => println!("error: {e}"),
                _ => println!("no answer from the sidecar"),
            }
            continue;
        }
        if provider == "ollama" && model.is_none() {
            println!("pick an Ollama model first: /models, then /model NAME");
            continue;
        }
        let ctx = context_for(&store.lock().unwrap());
        let backend = Backend { provider: provider.clone(), base_url: ollama_url.clone(), model: model.clone(), think };
        client.send_turn(line, &ctx, &backend, &mode);
        // Stream events until done.
        let mut streaming_line = false;
        let mut last_step = String::new();
        loop {
            match client.recv_timeout(Duration::from_secs(600)) {
                Some(AgentEvent::AssistantDelta { text, .. }) => {
                    print!("{text}");
                    out.flush()?;
                    streaming_line = true;
                }
                Some(AgentEvent::ThinkingDelta { .. }) => {
                    // Reasoning streams as dots rather than prose, so it is visibly alive without
                    // burying the answer; the whole block prints once it is done.
                    print!(".");
                    out.flush()?;
                    streaming_line = true;
                }
                Some(AgentEvent::Thinking { text, .. }) => {
                    if streaming_line {
                        println!();
                        streaming_line = false;
                    }
                    let short: String = text.chars().take(300).collect();
                    println!("  [thinking] {}{}", short.replace('\n', " "), if text.chars().count() > 300 { "…" } else { "" });
                }
                Some(AgentEvent::AssistantMessage { text, .. }) => {
                    if !streaming_line {
                        println!("\nclaude> {text}");
                    } else {
                        println!();
                        streaming_line = false;
                    }
                }
                Some(AgentEvent::ToolCall { name, input, .. }) => {
                    if streaming_line {
                        println!();
                        streaming_line = false;
                    }
                    let short = serde_json::to_string(&input).unwrap_or_default();
                    println!("  [tool] {name} {}", short.chars().take(160).collect::<String>());
                }
                Some(AgentEvent::ToolResult { name, summary, .. }) => {
                    println!("  [result] {name}: {}", summary.lines().take(3).collect::<Vec<_>>().join(" | ").chars().take(200).collect::<String>());
                }
                Some(AgentEvent::Done { turns, cost_usd, .. }) => {
                    if streaming_line {
                        println!();
                    }
                    println!("  [done] turns={turns} cost={}", cost_usd.map(|c| format!("${c:.3}")).unwrap_or_else(|| "-".into()));
                    break;
                }
                Some(AgentEvent::PlanProposed { plan_id, plan }) => {
                    if streaming_line {
                        println!();
                        streaming_line = false;
                    }
                    print_plan(&plan);
                    // The turn is parked inside a tool call, so the answer is read here rather than
                    // at the top-level prompt — and a piped script can answer the same way.
                    print!("\napprove? [enter]=yes  s=stop  anything else = send back with that note\nplan> ");
                    out.flush()?;
                    let mut answer = String::new();
                    let (decision, notes) = if stdin.lock().read_line(&mut answer)? == 0 {
                        ("cancel", None)
                    } else {
                        match answer.trim() {
                            "" | "y" | "yes" | "/approve" => ("approve", None),
                            "s" | "stop" | "/stop" => ("cancel", None),
                            note => ("reject", Some(note.to_string())),
                        }
                    };
                    client.send_plan_decision(&plan_id, decision, notes.as_deref());
                }
                Some(AgentEvent::PlanResolved { decision, .. }) => {
                    if decision == "stale" {
                        println!("  [plan] that answer arrived too late");
                    }
                }
                Some(AgentEvent::Phase { phase }) => {
                    println!("  [phase] {phase}");
                }
                Some(AgentEvent::Todos { items }) => {
                    // The checklist is resent on every write; only a change of step is news.
                    let done = items.iter().filter(|t| t.status == "completed").count();
                    let now = items.iter().find(|t| t.status == "in_progress").map(|t| t.content.clone());
                    let line = match &now {
                        Some(c) => format!("  [{done}/{}] {c}", items.len()),
                        None if !items.is_empty() && done == items.len() => format!("  [{done}/{}] all steps done", items.len()),
                        None => String::new(),
                    };
                    if !line.is_empty() && line != last_step {
                        println!("{line}");
                        last_step = line;
                    }
                }
                Some(AgentEvent::Error { message, .. }) => {
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

/// The `models` answer to a `list_models` request. Anything else that arrives meanwhile (a late
/// pong, a phase frame) is not what was asked for and is skipped.
fn wait_for_models(client: &AgentClient) -> Option<AgentEvent> {
    let deadline = std::time::Instant::now() + Duration::from_secs(8);
    while std::time::Instant::now() < deadline {
        match client.recv_timeout(Duration::from_millis(200)) {
            Some(ev @ AgentEvent::Models { .. }) => return Some(ev),
            Some(AgentEvent::Disconnected { .. }) => return None,
            _ => {}
        }
    }
    None
}

/// Prints a proposed plan as the steps it will take, so approving is answering a list of changes
/// rather than a paragraph.
fn print_plan(plan: &Plan) {
    println!("\n--- plan (revision {}) ---", plan.revision.max(1));
    if !plan.summary.is_empty() {
        println!("{}", plan.summary);
    }
    let mut facts: Vec<String> = Vec::new();
    if let Some(k) = &plan.key { facts.push(k.clone()); }
    if let Some(t) = plan.tempo { facts.push(format!("{t:.0} BPM")); }
    if let Some(ts) = &plan.time_signature { facts.push(ts.clone()); }
    if let Some(sty) = &plan.style { facts.push(sty.clone()); }
    if !facts.is_empty() {
        println!("{}", facts.join(" | "));
    }
    if !plan.form.is_empty() {
        println!("form: {}", plan.form.iter().map(|s| format!("{} x{}", s.name, s.bars)).collect::<Vec<_>>().join(" - "));
    }
    for (i, step) in plan.steps.iter().enumerate() {
        println!("{:>2}. [{}] {} -> {}", i + 1, step.owner, step.title, step.targets.join(", "));
        if !step.detail.is_empty() {
            println!("    {}", step.detail);
        }
    }
    for risk in &plan.risks {
        println!(" !  {risk}");
    }
}
