//! A scripted model server, for driving a real agent client through one
//! tool call without a model.
//!
//! The live suites (`live_<client>.rs` in `ff-cli`) point a client's base
//! URL here and read back what it sent: the proof that the briefing
//! reached the prompt is the request body, and the proof that the
//! client's hooks captured is the tool output the client carried back on
//! the next turn — `ff op log --json`, whose top entry is the capture the
//! hook took. Nothing here judges prose; the server answers every model
//! request by a fixed rule and records every one.
//!
//! Three dialects, told apart by the path the client posts to: the
//! OpenAI Responses API (`/responses`, Codex), OpenAI chat completions
//! (`/chat/completions`, Qwen Code and OpenCode), and Anthropic
//! messages (`/messages`, Claude Code — SSE when the body asks to
//! stream, one JSON object when it does not, since Claude Code retries
//! a failed stream unstreamed). Each dialect answers with a call to its
//! shell tool — the chat dialect names the tool the request offers,
//! OpenCode's `bash` or Qwen's `run_shell_command` — running the next of
//! the commands the server was started with, and once the last has run,
//! with the text `done`.
//!
//! The rule is stateless per request on purpose: the number of tool
//! outputs a body carries picks the command, the last command repeating
//! until its marker shows, and a body that carries the marker gets the
//! text. A client's retry of the same request, or an extra request after
//! the turn (Qwen Code extracts memories on its way out), cannot
//! desynchronize a counter that does not exist. The marker is the
//! caller's — fufu's suites pass `"cmd":"op log"`, the head of `ff op
//! log --json`'s envelope, which is only in a body once the last command
//! ran — matched in the escaped form a JSON string carries as well as
//! bare. A body carrying three more tool outputs than commands and
//! still no marker gets the text too: a command is failing in the
//! client's shell, and a turn that ends lets the suite's assertions say
//! so, where one more tool call would loop until the deadline.
//!
//! Dependency-free like the rest of the crate: `std::net` on a thread,
//! JSON as string literals, and one escaper for the command, whose
//! backslashes on Windows are the only thing that needs escaping.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// One recorded POST: the path as requested, the raw header block, and
/// the body as sent.
#[derive(Debug, Clone)]
pub struct Recorded {
    pub path: String,
    pub headers: String,
    pub body: String,
}

/// The server. Bound on start, served until dropped.
pub struct MockModel {
    addr: SocketAddr,
    recorded: Arc<Mutex<Vec<Recorded>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// A tool output in each dialect's body: the Responses item, the
/// Anthropic block, the chat message role.
const TOOL_OUTPUTS: [&str; 3] = [
    r#""type":"function_call_output""#,
    r#""type":"tool_result""#,
    r#""role":"tool""#,
];

/// How many failed attempts at the last command end the turn without
/// the marker.
const ATTEMPTS: usize = 3;

impl MockModel {
    /// Bind `127.0.0.1:0` and serve until dropped. `commands` are the
    /// shell lines the turns ask the client to run, one per tool call in
    /// order, and `marker` a substring of the last one's output that no
    /// request body carries until it has run — the head of a JSON
    /// envelope, say — which is what turns the tool call into the text.
    pub fn start(commands: &[&str], marker: &str) -> MockModel {
        assert!(!commands.is_empty(), "at least one command");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind the mock model");
        let addr = listener.local_addr().expect("a local address");
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let script = Arc::new(Script::new(commands, marker));
        let thread = {
            let recorded = Arc::clone(&recorded);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    let Ok(stream) = stream else { continue };
                    let recorded = Arc::clone(&recorded);
                    let script = Arc::clone(&script);
                    // One thread per connection: a client holds a
                    // keep-alive connection open across turns, and may
                    // open a second while the first idles.
                    std::thread::spawn(move || serve(stream, &recorded, &script));
                }
            })
        };
        MockModel {
            addr,
            recorded,
            stop,
            thread: Some(thread),
        }
    }

    /// `http://127.0.0.1:<port>`, with no path: each client adds its own
    /// `/v1` or not.
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// Every POST so far, in arrival order.
    pub fn requests(&self) -> Vec<Recorded> {
        self.recorded.lock().expect("the record").clone()
    }
}

impl Drop for MockModel {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // `accept` blocks; one connection wakes it to see the flag.
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The three dialects' answers, built once per command, and the marker
/// of a body that carries the last command's output, in the two forms
/// it can take: escaped inside a JSON string, which is how every client
/// today carries a tool result, and bare, for one that nests the output
/// as an object.
struct Script {
    commands: Vec<String>,
    markers: [String; 2],
    responses_calls: Vec<String>,
    responses_done: String,
    chat_done: String,
    messages_calls: Vec<String>,
    messages_done: String,
    messages_calls_json: Vec<String>,
    messages_done_json: String,
}

impl Script {
    fn new(commands: &[&str], marker: &str) -> Script {
        Script {
            commands: commands.iter().map(|c| c.to_string()).collect(),
            markers: [marker.replace('"', r#"\""#), marker.to_string()],
            responses_calls: commands
                .iter()
                .enumerate()
                .map(|(n, c)| responses_sse(&function_call_item(c, n)))
                .collect(),
            responses_done: responses_sse(ASSISTANT_MESSAGE_ITEM),
            chat_done: chat_sse(r#"{"role":"assistant","content":"done"}"#, "stop"),
            messages_calls: commands
                .iter()
                .enumerate()
                .map(|(n, c)| messages_sse(Some((c, n))))
                .collect(),
            messages_done: messages_sse(None),
            messages_calls_json: commands
                .iter()
                .enumerate()
                .map(|(n, c)| messages_json(&tool_use_block(c, n), "tool_use"))
                .collect(),
            messages_done_json: messages_json(TEXT_BLOCK, "end_turn"),
        }
    }

    /// The chat dialect's tool call, naming the tool the request offers:
    /// `bash` when the body declares one (OpenCode), else Qwen Code's
    /// `run_shell_command`. The arguments are `{"command": …}` either
    /// way. Every dialect's call id carries the command's index: a client
    /// keys tool results by id, and two calls under one id would leave
    /// one of them out of the next request.
    fn chat_call(&self, body: &str, n: usize) -> String {
        let name = if body.contains(r#""name":"bash""#) {
            "bash"
        } else {
            "run_shell_command"
        };
        chat_sse(
            &format!(
                r#"{{"role":"assistant","content":null,"tool_calls":[{{"index":0,"id":"call_{n}","type":"function","function":{{"name":"{name}","arguments":{}}}}}]}}"#,
                json_string(&format!(
                    r#"{{"command":{}}}"#,
                    json_string(&self.commands[n])
                ))
            ),
            "tool_calls",
        )
    }

    /// Which command the body is due, by the tool outputs it carries:
    /// the n-th, and the last again while its marker is missing; `None`
    /// once the marker shows or the last has failed `ATTEMPTS` times.
    fn due(&self, body: &str) -> Option<usize> {
        if self.markers.iter().any(|marker| body.contains(marker)) {
            return None;
        }
        let outputs: usize = TOOL_OUTPUTS
            .iter()
            .map(|marker| body.matches(marker).count())
            .sum();
        let last = self.commands.len() - 1;
        if outputs >= last + ATTEMPTS {
            return None;
        }
        Some(outputs.min(last))
    }

    /// The answer for one POST: the body and its content type, or
    /// `None` for a path no dialect claims.
    fn answer(&self, path: &str, body: &str) -> Option<(String, &str)> {
        let due = self.due(body);
        let path = path.split('?').next().unwrap_or(path);
        if path.ends_with("/responses") {
            return Some((
                match due {
                    None => self.responses_done.clone(),
                    Some(n) => self.responses_calls[n].clone(),
                },
                SSE,
            ));
        }
        if path.ends_with("/chat/completions") {
            return Some((
                match due {
                    None => self.chat_done.clone(),
                    Some(n) => self.chat_call(body, n),
                },
                SSE,
            ));
        }
        if path.contains("/messages") {
            return Some(if body.contains(r#""stream":true"#) {
                (
                    match due {
                        None => self.messages_done.clone(),
                        Some(n) => self.messages_calls[n].clone(),
                    },
                    SSE,
                )
            } else {
                (
                    match due {
                        None => self.messages_done_json.clone(),
                        Some(n) => self.messages_calls_json[n].clone(),
                    },
                    JSON,
                )
            });
        }
        None
    }
}

const SSE: &str = "text/event-stream";
const JSON: &str = "application/json";

/// The token counts Codex insists on in `response.completed`.
const USAGE: &str = r#"{"input_tokens":1,"output_tokens":1,"total_tokens":2,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}"#;

const ASSISTANT_MESSAGE_ITEM: &str = r#"{"type":"message","id":"msg_1","role":"assistant","status":"completed","content":[{"type":"output_text","text":"done","annotations":[]}]}"#;

const TEXT_BLOCK: &str = r#"{"type":"text","text":"done"}"#;

fn function_call_item(command: &str, n: usize) -> String {
    format!(
        r#"{{"type":"function_call","id":"fc_{n}","call_id":"call_{n}","name":"exec_command","arguments":{}}}"#,
        json_string(&format!(r#"{{"cmd":{}}}"#, json_string(command)))
    )
}

fn tool_use_block(command: &str, n: usize) -> String {
    format!(
        r#"{{"type":"tool_use","id":"toolu_{n}","name":"Bash","input":{{"command":{}}}}}"#,
        json_string(command)
    )
}

/// One Responses API stream carrying one output item: created, the
/// item done, completed.
fn responses_sse(item: &str) -> String {
    let created =
        r#"{"id":"resp_1","object":"response","created_at":0,"status":"in_progress","output":[]}"#;
    let completed = format!(
        r#"{{"id":"resp_1","object":"response","created_at":0,"status":"completed","output":[{item}],"usage":{USAGE}}}"#
    );
    sse(&[
        format!(r#"{{"type":"response.created","response":{created}}}"#),
        format!(r#"{{"type":"response.output_item.done","output_index":0,"item":{item}}}"#),
        format!(r#"{{"type":"response.completed","response":{completed}}}"#),
    ])
}

/// One chat completions stream: a single chunk carrying the whole
/// delta, then the terminator.
fn chat_sse(delta: &str, finish: &str) -> String {
    format!(
        "data: {{\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"created\":0,\"model\":\"mock\",\"choices\":[{{\"index\":0,\"delta\":{delta},\"finish_reason\":\"{finish}\"}}]}}\n\ndata: [DONE]\n\n"
    )
}

/// One Anthropic messages stream carrying one content block: a `Bash`
/// call running the n-th command, or the text `done` without one. A tool
/// block starts empty and its input arrives as one `input_json_delta`,
/// the way the real API sends it.
fn messages_sse(command: Option<(&str, usize)>) -> String {
    let (start, delta, stop) = match command {
        Some((command, n)) => (
            format!(r#"{{"type":"tool_use","id":"toolu_{n}","name":"Bash","input":{{}}}}"#),
            format!(
                r#"{{"type":"input_json_delta","partial_json":{}}}"#,
                json_string(&format!(r#"{{"command":{}}}"#, json_string(command)))
            ),
            "tool_use",
        ),
        None => (
            r#"{"type":"text","text":""}"#.to_string(),
            r#"{"type":"text_delta","text":"done"}"#.to_string(),
            "end_turn",
        ),
    };
    sse(&[
        r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"mock","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":1}}}"#.to_string(),
        format!(r#"{{"type":"content_block_start","index":0,"content_block":{start}}}"#),
        format!(r#"{{"type":"content_block_delta","index":0,"delta":{delta}}}"#),
        r#"{"type":"content_block_stop","index":0}"#.to_string(),
        format!(
            r#"{{"type":"message_delta","delta":{{"stop_reason":"{stop}","stop_sequence":null}},"usage":{{"output_tokens":1}}}}"#
        ),
        r#"{"type":"message_stop"}"#.to_string(),
    ])
}

/// Named events, one per object, the name read off the object's `type`.
fn sse(events: &[String]) -> String {
    events
        .iter()
        .map(|event| {
            let name = event
                .split("\"type\":\"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .expect("an event type");
            format!("event: {name}\ndata: {event}\n\n")
        })
        .collect()
}

/// The unstreamed Anthropic answer: one message object.
fn messages_json(block: &str, stop: &str) -> String {
    format!(
        r#"{{"id":"msg_1","type":"message","role":"assistant","model":"mock","content":[{block}],"stop_reason":"{stop}","stop_sequence":null,"usage":{{"input_tokens":1,"output_tokens":1}}}}"#
    )
}

/// `text` as a JSON string literal, quotes included.
pub fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// One connection: requests in sequence until the client closes it.
fn serve(stream: TcpStream, recorded: &Mutex<Vec<Recorded>>, script: &Script) {
    // An idle keep-alive connection is the client's to close; the
    // timeout is for one a dead client left behind.
    let _ = stream.set_read_timeout(Some(Duration::from_secs(300)));
    let mut reader = BufReader::new(stream);
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();

        let mut headers = String::new();
        let mut length: Option<usize> = None;
        loop {
            let mut header = String::new();
            match reader.read_line(&mut header) {
                Ok(0) | Err(_) => return,
                Ok(_) => {}
            }
            if header == "\r\n" || header == "\n" {
                break;
            }
            if let Some((name, value)) = header.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                length = value.trim().parse().ok();
            }
            headers.push_str(&header);
        }

        let (status, content_type, body) = match (method.as_str(), length) {
            ("POST", Some(length)) => {
                let mut raw = vec![0; length];
                if reader.read_exact(&mut raw).is_err() {
                    return;
                }
                let body = String::from_utf8_lossy(&raw).into_owned();
                let answer = script.answer(&path, &body);
                recorded.lock().expect("the record").push(Recorded {
                    path: path.clone(),
                    headers: headers.clone(),
                    body,
                });
                match answer {
                    Some((body, content_type)) => ("200 OK", content_type, body),
                    None => ("404 Not Found", JSON, "{}".to_string()),
                }
            }
            // A body framed any other way — chunked — is not read: it
            // is recorded empty, so the suite's body assertions name it.
            ("POST", None) => {
                recorded.lock().expect("the record").push(Recorded {
                    path: path.clone(),
                    headers: headers.clone(),
                    body: String::new(),
                });
                ("411 Length Required", JSON, "{}".to_string())
            }
            _ => ("404 Not Found", JSON, "{}".to_string()),
        };
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
            body.len()
        );
        let stream = reader.get_mut();
        if stream.write_all(head.as_bytes()).is_err() || stream.write_all(body.as_bytes()).is_err()
        {
            return;
        }
        let _ = stream.flush();
        if status.starts_with("411") {
            // The unread body would be parsed as the next request line.
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One POST over a raw socket, `Connection: close` so the response
    /// is read to EOF.
    fn post(model: &MockModel, path: &str, body: &str) -> (u16, String, String) {
        let mut stream = TcpStream::connect(model.addr).expect("connect");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        let head = format!(
            "POST {path} HTTP/1.1\r\nHost: mock\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(body.as_bytes()).unwrap();
        // The server keeps the connection; the client's half-close ends
        // the read below.
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        let (head, body) = response.split_once("\r\n\r\n").expect("a response");
        let status: u16 = head
            .lines()
            .next()
            .and_then(|line| line.split_whitespace().nth(1))
            .and_then(|code| code.parse().ok())
            .expect("a status");
        (status, head.to_string(), body.to_string())
    }

    const CMD: &str = r#"C:\tools\ff op log --json -n 1"#;
    const MARKER: &str = r#""cmd":"op log""#;

    #[test]
    fn responses_gets_the_tool_call_then_the_text() {
        let model = MockModel::start(&[CMD], MARKER);
        let (status, head, body) = post(&model, "/v1/responses", r#"{"input":[],"stream":true}"#);
        assert_eq!(status, 200);
        assert!(head.contains("text/event-stream"), "{head}");
        assert!(body.starts_with("event: response.created\n"), "{body}");
        assert!(body.contains(r#""name":"exec_command""#), "{body}");
        assert!(
            body.contains(r#""arguments":"{\"cmd\":\"C:\\\\tools\\\\ff op log --json -n 1\"}""#),
            "{body}"
        );
        assert!(body.contains("event: response.completed\n"), "{body}");
        assert!(body.contains(r#""total_tokens":2"#), "{body}");

        let (_, _, body) = post(
            &model,
            "/v1/responses",
            r#"{"input":[{"type":"function_call_output","output":"{\"ff\":1,\"cmd\":\"op log\"}"}]}"#,
        );
        assert!(body.contains(r#""text":"done""#), "{body}");
        assert!(!body.contains("exec_command"), "{body}");
    }

    #[test]
    fn chat_completions_gets_the_tool_call_then_the_text() {
        let model = MockModel::start(&["ff op log --json -n 1"], MARKER);
        let (status, head, body) = post(&model, "/v1/chat/completions", r#"{"messages":[]}"#);
        assert_eq!(status, 200);
        assert!(head.contains("text/event-stream"), "{head}");
        assert!(body.contains(r#""name":"run_shell_command""#), "{body}");
        assert!(
            body.contains(r#""arguments":"{\"command\":\"ff op log --json -n 1\"}""#),
            "{body}"
        );
        assert!(body.contains(r#""finish_reason":"tool_calls""#), "{body}");
        assert!(body.ends_with("data: [DONE]\n\n"), "{body}");

        let (_, _, body) = post(
            &model,
            "/v1/chat/completions",
            r#"{"messages":[{"role":"tool","content":"{\"ff\":1,\"cmd\":\"op log\"}"}]}"#,
        );
        assert!(body.contains(r#""content":"done""#), "{body}");
        assert!(body.contains(r#""finish_reason":"stop""#), "{body}");
    }

    /// The chat call names the tool the request offers: a body
    /// declaring `bash` gets `bash`, and one declaring
    /// `run_shell_command` keeps that name.
    #[test]
    fn chat_completions_names_the_tool_the_request_offers() {
        let model = MockModel::start(&["ff op log --json -n 1"], MARKER);
        let (_, _, body) = post(
            &model,
            "/v1/chat/completions",
            r#"{"messages":[],"tools":[{"type":"function","function":{"name":"bash","parameters":{}}}]}"#,
        );
        assert!(body.contains(r#""name":"bash""#), "{body}");
        assert!(!body.contains("run_shell_command"), "{body}");
        assert!(
            body.contains(r#""arguments":"{\"command\":\"ff op log --json -n 1\"}""#),
            "{body}"
        );
        let (_, _, body) = post(
            &model,
            "/v1/chat/completions",
            r#"{"messages":[],"tools":[{"type":"function","function":{"name":"run_shell_command","parameters":{}}}]}"#,
        );
        assert!(body.contains(r#""name":"run_shell_command""#), "{body}");
    }

    #[test]
    fn messages_streams_or_not_as_the_body_asks() {
        let model = MockModel::start(&["ff op log --json -n 1"], MARKER);
        let (status, head, body) = post(&model, "/v1/messages?beta=true", r#"{"stream":true}"#);
        assert_eq!(status, 200);
        assert!(head.contains("text/event-stream"), "{head}");
        assert!(body.starts_with("event: message_start\n"), "{body}");
        assert!(
            body.contains(r#""id":"toolu_0","name":"Bash","input":{}"#),
            "{body}"
        );
        assert!(
            body.contains(r#""partial_json":"{\"command\":\"ff op log --json -n 1\"}""#),
            "{body}"
        );
        assert!(body.contains(r#""stop_reason":"tool_use""#), "{body}");

        let (status, head, body) = post(&model, "/v1/messages", r#"{"stream":false}"#);
        assert_eq!(status, 200);
        assert!(head.contains("application/json"), "{head}");
        assert!(body.starts_with(r#"{"id":"msg_1""#), "{body}");
        assert!(
            body.contains(r#""input":{"command":"ff op log --json -n 1"}"#),
            "{body}"
        );

        let (_, _, body) = post(
            &model,
            "/v1/messages",
            r#"{"stream":false,"messages":[{"content":"{\"ff\":1,\"cmd\":\"op log\"}"}]}"#,
        );
        assert!(body.contains(r#""stop_reason":"end_turn""#), "{body}");
        assert!(body.contains(r#""text":"done""#), "{body}");
    }

    #[test]
    fn requests_are_recorded_in_order_and_unknown_paths_are_404() {
        let model = MockModel::start(&["ff op log --json -n 1"], MARKER);
        post(&model, "/v1/responses", "first");
        let (status, _, _) = post(&model, "/v1/models", "second");
        assert_eq!(status, 404);
        post(&model, "/v1/messages", "third");
        let recorded = model.requests();
        let bodies: Vec<&str> = recorded.iter().map(|r| r.body.as_str()).collect();
        assert_eq!(bodies, ["first", "second", "third"]);
        assert_eq!(recorded[1].path, "/v1/models");
        assert!(recorded[0].headers.contains("Content-Length: 5"));
    }

    #[test]
    fn a_command_that_keeps_failing_ends_the_turn() {
        let model = MockModel::start(&["ff op log --json -n 1"], MARKER);
        let failed =
            r#"{"type":"function_call_output","call_id":"call_1","output":"bwrap: no permission"}"#;
        let two = format!(r#"{{"input":[{failed},{failed}]}}"#);
        let (_, _, body) = post(&model, "/v1/responses", &two);
        assert!(
            body.contains("exec_command"),
            "two failures try again: {body}"
        );
        let three = format!(r#"{{"input":[{failed},{failed},{failed}]}}"#);
        let (_, _, body) = post(&model, "/v1/responses", &three);
        assert!(!body.contains("exec_command"), "three end the turn: {body}");
        assert!(body.contains(r#""text":"done""#), "{body}");
    }

    /// Two commands: a body with no tool output gets the first, one
    /// with one output gets the second, and the second repeats until its
    /// marker shows.
    #[test]
    fn the_commands_run_in_order_and_the_last_repeats() {
        let model = MockModel::start(&["touch b.txt", "ff op log --json -n 1"], MARKER);
        let (_, _, body) = post(&model, "/v1/responses", r#"{"input":[]}"#);
        assert!(body.contains(r#"touch b.txt"#), "{body}");
        assert!(!body.contains("op log"), "{body}");
        let one = r#"{"input":[{"type":"function_call_output","call_id":"call_1","output":""}]}"#;
        let (_, _, body) = post(&model, "/v1/responses", one);
        assert!(body.contains("op log"), "{body}");
        let two = r#"{"input":[{"type":"function_call_output","output":""},{"type":"function_call_output","output":"bwrap: no"}]}"#;
        let (_, _, body) = post(&model, "/v1/responses", two);
        assert!(body.contains("op log"), "the last repeats: {body}");
        let done = r#"{"input":[{"type":"function_call_output","output":""},{"type":"function_call_output","output":"{\"ff\":1,\"cmd\":\"op log\"}"}]}"#;
        let (_, _, body) = post(&model, "/v1/responses", done);
        assert!(body.contains(r#""text":"done""#), "{body}");
    }

    #[test]
    fn json_string_escapes() {
        assert_eq!(json_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(json_string("x\ny"), r#""x\ny""#);
    }
}
