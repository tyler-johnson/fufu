//! `ff mcp`: fufu's verbs as typed tools over the Model Context Protocol,
//! on stdio.
//!
//! The server is a shell over the machine surface and nothing more. It
//! serves seven typed tools — `status`, `pull`, `push`, `undo`, `redo`,
//! `explain`, and `help` — for the verbs where the shell adds nothing:
//! fixed and short inputs, no output an agent would pipe, and a result
//! whose structure matters more than its text. Each takes the verb's own
//! flags as fields and a `cwd`, and every call runs this same binary as a
//! child with `--json` and hands the envelope back. Capture-first, the git
//! policy, sessions, error ids, and the no-prompt guarantee all hold
//! because the child is an ordinary invocation; the server decides
//! nothing about any of them. That is what DESIGN.md promises of any
//! further surface — a thin shell over one contract rather than a second
//! implementation with its own opinions. Every other verb is the shell.
//!
//! Beside the seven, the tools a declared extension produced. An extension
//! promising `tools` in its manifest is asked what they are when the
//! server starts, and each answer is listed under `<extension>__<tool>`
//! and routed through the same child. `verbs.rs` generates fufu's seven
//! from the command tree, and `tools.rs` has the fetch, the names, and the
//! one spelling of an arguments object into a command line that both go
//! through.
//!
//! The protocol has two handshake eras. Revisions through 2025-11-25 open
//! with an `initialize` exchange and hold a session; 2026-07-28 dropped
//! the handshake, made `server/discover` mandatory, and carries the
//! version in every request's `_meta`. The SDK serves both from one
//! handler, and the tests drive both by hand.
//!
//! Stdout belongs to the protocol: every byte on it is a frame the client
//! parses, which is why the verb is not `--json` capable and rides no
//! lanes. Stderr carries nothing unless `FF_DEBUG=1`, the same rule the
//! trigger runtime keeps, because a client shows a server's stderr to
//! nobody and a line there is a line lost.

pub(crate) mod child;
mod tools;
mod verbs;

use std::path::PathBuf;

use ff_core::{Error, Result};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, Implementation, ListToolsResult,
    PaginatedRequestParams, ServerCapabilities, ServerInfo,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::{ErrorData, ServerHandler, ServiceExt};

use crate::ctx::Ctx;

/// What every call needs and the client never sends: which binary to run
/// and which session its operations carry.
struct Server {
    /// This binary, by absolute path, resolved once at start. The
    /// sanctioned self-spawn precedent is the update check; neither goes
    /// through `PATH`, because the `ff` on the client's `PATH` may not be
    /// the one serving.
    exe: PathBuf,
    /// The session `Ctx` settled for `ff mcp` itself — `--session`, then
    /// `FF_SESSION`, then the session the client says it launched this
    /// server under, the precedence every invocation has. Server-level only:
    /// a tag per call would be a second session mechanism to explain.
    session: Option<String>,
    /// What is served: fufu's seven first, then the tools declared
    /// extensions produced, asked for once and served for the life of the
    /// connection — the way the registry itself is read once, and for the
    /// same reason: what was advertised at handshake is what answers until
    /// the client closes.
    tools: Vec<tools::Typed>,
}

pub fn run(ctx: &Ctx) -> Result<()> {
    let exe = std::env::current_exe().map_err(Error::repo)?;
    // Before the transport, so the first `tools/list` is answered from a
    // list that is already there. Each ask of an extension is time-boxed,
    // and a machine that has declared nothing spawns nothing.
    let mut served = verbs::own();
    tools::produced(&mut served, crate::registry::read());
    let server = Server {
        exe,
        session: ctx.session.clone(),
        tools: served,
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::repo)?;
    runtime.block_on(async move {
        let running = match server.serve(rmcp::transport::stdio()).await {
            Ok(running) => running,
            // The client closed the pipe before saying anything, which is
            // how a client probes whether a server starts at all. Nothing
            // went wrong, so nothing is reported.
            Err(rmcp::service::ServerInitializeError::ConnectionClosed(_)) => return Ok(()),
            Err(err) => return Err(complain(&err)),
        };
        match running.waiting().await {
            Ok(_) => Ok(()),
            Err(err) => Err(complain(&err)),
        }
    })
}

/// A server failure is a line on stderr only under `FF_DEBUG`, and an exit
/// code otherwise; the client is the only reader, and it renders none of
/// this.
fn complain(err: &dyn std::fmt::Display) -> Error {
    if std::env::var_os("FF_DEBUG").is_some() {
        eprintln!("ff[debug]: mcp: {err}");
    }
    Error::msg(format!("mcp: {err}"))
}

/// What to say about a name nothing here answers to. The seven by name and
/// the produced tools by shape, since a registry is a person's file and
/// naming every produced tool would be a message as long as one.
fn unknown(name: &str) -> String {
    format!(
        "no tool named {name:?}; this server serves status, pull, push, undo, redo, explain, \
         help, and the tools a declared extension produced, each named <extension>__<tool>"
    )
}

impl ServerHandler for Server {
    /// The instructions field carries the briefing, the same notice the
    /// hook injects, plus the tools line the hook adds only where a server
    /// is registered: a server that is answering is offered by definition.
    /// A client that surfaces instructions has the doctrine, and one that
    /// does not has the tools' own descriptions.
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                crate::cli::NAME,
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(format!(
                "{}{}",
                crate::integ::briefing::NOTICE,
                crate::integ::mcp::LINE
            ))
    }

    /// fufu's seven first, then a tool per descriptor a declared extension
    /// produced, in the order the registry declared them.
    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListToolsResult, ErrorData>> + Send + '_ {
        let listed = self.tools.iter().map(tools::Typed::tool).collect();
        std::future::ready(Ok(ListToolsResult::with_all_items(listed)))
    }

    /// `Err` only for a name nothing serves or input the tool's schema
    /// already forbids — a `cwd` that is not a string, a value a command
    /// line has no spelling for. A fufu failure is a *successful* tool call
    /// carrying `is_error`, because a client renders a JSON-RPC error
    /// opaquely and the envelope inside it is what the agent needs to read.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        let Some(typed) = self.tools.iter().find(|typed| typed.name() == request.name) else {
            return Err(ErrorData::invalid_params(unknown(&request.name), None));
        };
        let call = typed.call(request.arguments)?;
        Ok(child::run(&self.exe, self.session.as_deref(), call)
            .await
            .into())
    }
}
