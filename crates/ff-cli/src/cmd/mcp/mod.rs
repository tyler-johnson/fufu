//! `ff mcp`: fufu as one tool over the Model Context Protocol, on stdio.
//!
//! The server is a shell over the machine surface and nothing more. It
//! exposes a single tool, `ff`, whose input is the command line after `ff`
//! as an array of words, and every call runs this same binary as a child
//! with `--json` and relays the envelope back. Capture-first, the git
//! policy, sessions, error ids, and the no-prompt guarantee all hold
//! because the child is an ordinary invocation; the server decides
//! nothing about any of them. That is what DESIGN.md promises of any
//! further surface — a thin shell over one contract rather than a second
//! implementation with its own opinions.
//!
//! One tool rather than one per verb, because a client transmits every
//! tool's description on every turn, and shows the model only the first
//! two thousand characters or so of each. The one description is a card
//! under that cut — the contract, the doctrine, every verb by name, and a
//! digest of recovery and the landmines — where forty typed tools would be
//! forty cards and a second spelling of the CLI to keep in step.
//! `describe.rs` assembles the verb list from the same source `ff --help`
//! reads, so it cannot drift from it.
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
//!
//! While it serves, the server holds a presence marker under the user's
//! cache directory, keyed by the client process that spawned it and by the
//! name the server is registered under. That is
//! what lets `ff trigger claude` refuse `ff` in the shell under
//! `fufu.toolPolicy` only when this tool is actually up for the client
//! making the call — `presence.rs` has the mechanism.

pub(crate) mod child;
pub mod describe;
pub mod presence;

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
    /// `--session` on `ff mcp`, or `FF_SESSION` in its environment, already
    /// settled by `Ctx` with the flag winning. Server-level only: a tag per
    /// call would be a second session mechanism to explain.
    session: Option<String>,
}

pub fn run(ctx: &Ctx) -> Result<()> {
    let exe = std::env::current_exe().map_err(Error::repo)?;
    let server = Server {
        exe,
        session: ctx.session.clone(),
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
        // Up, and provably so: the marker is held for as long as this
        // serves, and the hook reads it to decide whether `ff` in the
        // shell is refused. After `serve` on purpose, so the probe path
        // above writes nothing.
        let _held = presence::hold();
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

impl ServerHandler for Server {
    /// The instructions field carries the short doctrine only. The rich
    /// text goes on the tool, because every client transmits tool
    /// descriptions and not every client surfaces instructions.
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                crate::cli::NAME,
                env!("CARGO_PKG_VERSION"),
            ))
            .with_instructions(crate::integ::briefing::NOTICE)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListToolsResult, ErrorData>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(vec![describe::tool()])))
    }

    /// `Err` only for input the schema already forbids — no `args`, an item
    /// that is not a string. A fufu failure is a *successful* tool call
    /// carrying `is_error`, because a client renders a JSON-RPC error
    /// opaquely and the envelope inside it is what the agent needs to read.
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResponse, ErrorData> {
        if request.name != describe::NAME {
            return Err(ErrorData::invalid_params(
                format!(
                    "no tool named {:?}; the one tool is {:?}",
                    request.name,
                    describe::NAME
                ),
                None,
            ));
        }
        let call = child::parse(request.arguments)?;
        Ok(child::run(&self.exe, self.session.as_deref(), call)
            .await
            .into())
    }
}
