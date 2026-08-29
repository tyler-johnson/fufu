//! Human-readable rendering: plain rows, no TUI; the log family carries
//! ANSI color when the stream says so (see the palette below).

mod age;
mod diff;
mod palette;
mod rows;
mod status;

pub use age::relative_age;
pub(crate) use diff::{patch_block, render_diffstat};
pub use palette::{
    col, init_palette, paint_dim, paint_id, paint_ok, paint_sha, paint_warn, styled_id,
};
pub use rows::{
    ChangeRowDisplay, CommitRowDisplay, MapPayload, branch_label_width, branch_row, change_row,
    commit_row, history_row, log_row, map_payload, op_row, remote_branch_row, remote_label_width,
    remote_more_row, snap_row,
};
pub use status::{StatusView, reconcile_notice, status_human};
pub(crate) use status::{dropped_line, held_block};
