//! `ff collide` — the sideways axis: would two branches, neither beneath
//! the other, collide with each other?
//!
//! The other two axes measure one branch against the base beneath it, or
//! against the remote copy of itself — a branch measured vertically. This
//! verb asks what those cannot. The answer is read through the ordinary
//! `ff_core::discover` handle, the probe writes nothing to the object
//! database, and a collision is a finding, not a failure — the exit is 0.

use ff_core::{Error, Pairing, Result, Side};

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, names: Vec<String>) -> Result<()> {
    // Reading the sideways axis as of a past operation would need the
    // past-state view that does not exist yet — the same refusal `ff remote`
    // makes for its rows.
    ctx.refuse_past("ff collide")?;

    let repo = ff_core::discover(".")?;

    // One name is the branch you are on against that one; two names say
    // both sides. Clap's `num_args = 1..=2` refuses every other count.
    let (a, b) = match names.as_slice() {
        [b] => (current_branch(&repo)?, b.clone()),
        [a, b] => (a.clone(), b.clone()),
        _ => unreachable!("clap bounds the names at one or two"),
    };

    let collision = ff_core::collide(&repo, &a, &b)?;

    if ctx.json {
        // `Collision` serializes itself: `{"a": {...}, "b": {...},
        // "pairing": {...}}` — an object with somewhere to grow.
        return crate::machine::emit("collide", &collision);
    }

    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    // Neither name is a palette role — the palette colors roles, and a
    // branch name is one of the things fufu names, not a role it paints.
    // The glyph carries the meaning; the color is redundant, so under
    // `NO_COLOR` the three cases still separate.
    let (glyph, tail) = match &collision.pairing {
        Pairing::Clear => (crate::render::paint_ok("✓", colored), None),
        Pairing::Collide { paths } => {
            let shown = paths.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
            let extra = paths.len().saturating_sub(3);
            let mut line = shown;
            if extra > 0 {
                line.push_str(&format!(" and {extra} more"));
            }
            (crate::render::paint_warn("✕", colored), Some(line))
        }
        Pairing::Unknown { reason } => (
            crate::render::paint_dim("?", colored),
            Some(crate::render::paint_dim(
                &format!("can't compare ({})", reason.text()),
                colored,
            )),
        ),
    };

    // A side that carries uncommitted work is suffixed `*`; the legend is
    // printed once, only when earned.
    let line = format!(
        "  {}  {} {}",
        marked(&collision.a),
        glyph,
        marked(&collision.b)
    );
    let line = match tail {
        Some(tail) => format!("{line}  {tail}"),
        None => line,
    };
    println!("{}", line.trim_end());

    if collision.a.open || collision.b.open {
        println!();
        println!(
            "{}",
            crate::render::paint_dim("  * has uncommitted work", colored)
        );
    }
    Ok(())
}

/// The name a side wears: its own, plus `*` when the tree judged was the
/// open change's rather than the tip's.
fn marked(side: &Side) -> String {
    if side.open {
        format!("{}*", side.name)
    } else {
        side.name.clone()
    }
}

/// The branch the one-name form compares against. An unborn branch has a
/// name and no tip, which `ff_core::collide` refuses by that name; a
/// detached HEAD has no name to refuse by, so it is refused here.
fn current_branch(repo: &gix::Repository) -> Result<String> {
    match ff_core::head_state(repo)? {
        ff_core::HeadState::Branch { name, .. } => Ok(name),
        ff_core::HeadState::Unborn { r#ref } => Ok(r#ref
            .strip_prefix("refs/heads/")
            .unwrap_or(&r#ref)
            .to_string()),
        ff_core::HeadState::Detached { .. } => Err(Error::coded(
            "repo/detached",
            "detached HEAD: there is no branch here to compare",
            vec!["ff collide <a> <b>".into(), "ff switch <branch>".into()],
        )),
    }
}
