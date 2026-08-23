//! `ff collide` — the sideways axis: would two branches, neither beneath
//! the other, collide with each other?
//!
//! The other two axes measure one branch against the base beneath it, or
//! against the remote copy of itself — a branch measured vertically. This
//! verb asks what those cannot. The answer is read through the ordinary
//! `ff_core::discover` handle, the probe writes nothing to the object
//! database, and a collision is a finding, not a failure — the exit is 0.

use ff_core::Pairing;
use ff_core::Result;

use crate::ctx::Ctx;

pub fn run(ctx: &Ctx, names: Vec<String>, branches: Option<usize>, all: bool) -> Result<()> {
    // Reading the sideways axis as of a past operation would need the
    // past-state view that does not exist yet — the same refusal `ff remote`
    // makes for its rows.
    ctx.refuse_past("ff collide")?;

    let repo = ff_core::discover(".")?;

    // 0 means all; `--all` is the same wish spelled out.
    let limit = if all {
        None
    } else {
        match branches {
            None => Some(10),
            Some(0) => None,
            Some(n) => Some(n),
        }
    };

    let collisions = ff_core::collide::collide(
        &repo,
        &ff_core::CollideOptions {
            branches: limit,
            names,
        },
    )?;

    if ctx.json {
        // The `Collisions` serializes itself: `{"sides": [...], "pairs":
        // [...], "clear": [...]}` — an object with somewhere to grow.
        return crate::machine::emit("collide", &collisions);
    }

    crate::render::init_palette(&repo);
    let colored = crate::pager::color_enabled();

    if collisions.sides.len() < 2 {
        // Fewer than two branches is a state, not a failure: the exit is 0.
        println!(
            "{}",
            crate::render::paint_dim("nothing to compare", colored)
        );
        return Ok(());
    }

    // A side that carries uncommitted work is suffixed `*` wherever its
    // name appears; the legend is printed once, only when earned.
    let open: Vec<&String> = collisions
        .sides
        .iter()
        .filter(|side| side.open)
        .map(|side| &side.name)
        .collect();
    let marked = |name: &str| {
        if open.iter().any(|open| *open == name) {
            format!("{name}*")
        } else {
            name.to_string()
        }
    };

    // Neither name is a palette role — the palette colors roles, and a
    // branch name is one of the things fufu names, not a role it paints.
    // The glyph carries the meaning; the color is redundant, so under
    // `NO_COLOR` the three cases still separate.
    let width = collisions
        .sides
        .iter()
        .map(|side| marked(&side.name).chars().count())
        .max()
        .unwrap_or_default();
    for pair in &collisions.pairs {
        let (glyph, tail) = match &pair.pairing {
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
        let line = format!(
            "  {:<width$}  {} {:<width$}",
            marked(&pair.a),
            glyph,
            marked(&pair.b)
        );
        let line = match tail {
            Some(tail) => format!("{line}  {tail}"),
            None => line,
        };
        println!("{}", line.trim_end());
    }

    if !open.is_empty() {
        println!();
        println!(
            "{}",
            crate::render::paint_dim("  * has uncommitted work", colored)
        );
    }
    if collisions.clear.is_empty() {
        println!("{}", crate::render::paint_dim("  clear set: none", colored));
    } else {
        let clear = collisions
            .clear
            .iter()
            .map(|name| marked(name))
            .collect::<Vec<_>>()
            .join(", ");
        println!("  clear set: {clear}");
    }
    Ok(())
}
