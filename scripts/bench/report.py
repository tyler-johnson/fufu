#!/usr/bin/env python3
# Turns bench-results/raw.json (schema: scripts/bench/bench-schema.md, frozen
# alongside this file's brief) into a pass/fail gate plus a readable table.
#
# fufu's performance claim is "no linear-time costs": a command that is 6ms
# today is 6ms at 100x the history. hyperfine gives us absolute milliseconds,
# which are machine- and load-specific and noisy on shared CI runners; the
# ratio between the smallest and largest N on an axis is what survives that
# noise, because a constant multiplied by machine speed cancels out of a
# ratio. Process startup is itself a fixed cost that can hide a real linear
# term behind a flat-looking raw ratio (a 5ms floor plus a term going
# 0.1ms -> 1ms is a raw ratio of ~1.18), so every measurement is floor-
# subtracted before the ratio is taken. stdlib only: this runs on a
# Raspberry Pi and on ubuntu-latest CI, and must not depend on anything
# that needs a pip install.

import argparse
import itertools
import json
import math
import sys

DEFAULT_PATH = "bench-results/raw.json"
DEFAULT_FLAT_MAX = 1.5

# What a row's declared expectation may say. "flat" is the gate -- the row must
# not grow with its axis. "linear" is a row allowed to scale with its axis
# (scanning N files costs O(N) for every tool alive), recorded so a worse-than-
# linear turn is visible. "report" is a row measured purely for comparison,
# on an axis that is not enforcement -- the real-repo numbers that go in the
# README. Only "flat" ever fails a run.
EXPECT_VALUES = frozenset(("flat", "linear", "report"))

FRAMING_LINE = (
    "flatness is the claim: absolute milliseconds are secondary and "
    "machine-specific. The gated number is the floor-subtracted cost growth "
    "per 10x of N -- flat is ~1.0, linear is ~10 -- and it is compared "
    "against the flat max."
)

# ff first because it's the thing under test; git/jj are peers shown for
# context and sort after in a stable, always-the-same order.
TOOL_ORDER = {"ff": 0, "git": 1, "jj": 2}


def tool_sort_key(tool):
    return TOOL_ORDER.get(tool, 3)


def load_doc(path):
    if path == "-":
        return json.load(sys.stdin)
    with open(path) as f:
        return json.load(f)


def group_key(elem):
    return (elem["axis"], elem["row"], elem["tool"])


def analyze(doc, flat_max):
    meta = doc["meta"]
    results = doc["results"]

    floors = {}
    rows_by_group = {}

    for elem in results:
        kind = elem["kind"]
        if kind == "floor":
            floors[(elem["tool"], elem["axis"], elem["n"])] = elem
        elif kind == "row":
            key = group_key(elem)
            rows_by_group.setdefault(key, []).append(elem)
        else:
            raise ValueError("unknown result kind: %r" % (kind,))

    groups = []
    for (axis, row, tool), elems in rows_by_group.items():
        elems = sorted(elems, key=lambda e: e["n"])
        groups.append(analyze_group(axis, row, tool, elems, floors, flat_max))

    groups.sort(key=lambda g: (g["axis"], g["row"], tool_sort_key(g["tool"]), g["tool"]))
    return meta, groups


def analyze_group(axis, row, tool, elems, floors, flat_max):
    expect = elems[0]["expect"]
    command = elems[0]["command"]

    # Only "flat" is gated, so any value that isn't one of the three known ones
    # silently disables the gate for that row -- a typo in rows.tsv's expect
    # column would turn the check off and still print a green report, which is
    # the one outcome this suite exists to prevent. Refuse to run instead.
    if expect not in EXPECT_VALUES:
        raise ValueError(
            "unknown expect %r for %s/%s/%s (want one of: %s)"
            % (expect, row, axis, tool, ", ".join(sorted(EXPECT_VALUES)))
        )

    points = []
    for e in elems:
        points.append({"n": e["n"], "elem": e, "floor": floors.get((tool, axis, e["n"]))})

    g = {
        "axis": axis,
        "row": row,
        "tool": tool,
        "expect": expect,
        "command": command,
        "points": points,
        "verdict": None,
        "ratio": None,
        "ratio_per_decade": None,
        "decades": None,
        "consecutive_ratios": [],
        "notes": [],
        # gates: this group is one that the pass/fail decision is even
        # scoped to (flat-expected ff rows). always_fail short-circuits
        # that scoping: a missing floor means the run itself is broken,
        # not that this particular row scaled, so it fails regardless of
        # tool/expect -- see the "missing floor" note in the brief.
        "gates": expect == "flat" and tool == "ff",
        "always_fail": False,
        "conditional_fail": False,
        "fails": False,
    }

    # A command that failed to run has no timing worth trusting. Checked
    # before floor/ratio math so a crash never masquerades as a number.
    if any(not p["elem"]["exit_ok"] for p in points):
        g["verdict"] = "errored"
        bad = [str(p["n"]) for p in points if not p["elem"]["exit_ok"]]
        g["notes"].append("exit_ok=false at n=" + ",".join(bad))
        return g

    if any(p["floor"] is None for p in points):
        g["verdict"] = "no-floor"
        missing = [str(p["n"]) for p in points if p["floor"] is None]
        g["notes"].append("no floor measured for n=" + ",".join(missing))
        g["always_fail"] = True
        g["fails"] = True
        return g

    if len(points) < 2:
        g["verdict"] = "single-point"
        g["notes"].append("only one point measured; ratio needs at least two")
        return g

    clamped_at = []
    for p in points:
        raw = p["elem"]["mean_ms"] - p["floor"]["mean_ms"]
        p["adjusted_ms"] = raw if raw >= 0.0 else 0.0
        if raw < 0.0:
            clamped_at.append(p["n"])

    p_min, p_max = points[0], points[-1]
    adj_min, adj_max = p_min["adjusted_ms"], p_max["adjusted_ms"]
    floor_sd_min = p_min["floor"]["stddev_ms"]
    floor_sd_max = p_max["floor"]["stddev_ms"]

    # A floor whose own stddev rivals its mean means the box was too busy to
    # measure anything on: say so, rather than letting a wild floor quietly
    # reshape every verdict derived from it.
    for p in (p_min, p_max):
        f = p["floor"]
        if f["mean_ms"] > 0 and f["stddev_ms"] > 0.5 * f["mean_ms"]:
            g["notes"].append(
                "noisy floor at n=%d (%.2f+-%.2fms): treat this row as unreliable"
                % (p["n"], f["mean_ms"], f["stddev_ms"])
            )

    threshold_min = max(0.05, floor_sd_min)
    if adj_min < threshold_min:
        g["verdict"] = "below-floor"
        threshold_max = max(0.5, 4 * floor_sd_max)
        # Growth is required, not just a large-N cost above the floor. The two
        # ends are judged against their own point's floor stddev, so a run
        # where only the small-N floor was noisy makes threshold_min huge while
        # threshold_max stays small -- and every row, including ones that got
        # measurably FASTER, would trip the "linear term" branch on that
        # asymmetry alone. Observed doing exactly that: 2.47ms -> 2.24ms
        # reported as a suspected linear term. A cost that did not grow cannot
        # be linear in anything, whatever the floors did.
        if adj_max > threshold_max and adj_max > adj_min * flat_max:
            # Small-N cost vanished into the floor, but large-N did not --
            # that shape is a linear term hiding behind a noisy near-zero
            # baseline, not genuine flatness. Fails even though the ratio
            # itself is undefined/meaningless (denominator is noise).
            g["conditional_fail"] = True
            g["notes"].append(
                "adjusted(n=%d)=%.2fms is below the noise floor (<%.2fms) but "
                "adjusted(n=%d)=%.2fms exceeds the floor threshold (>%.2fms) "
                "-- linear term suspected"
                % (p_min["n"], adj_min, threshold_min, p_max["n"], adj_max, threshold_max)
            )
        else:
            g["notes"].append(
                "adjusted time at or below the noise floor at both n=%d and n=%d"
                % (p_min["n"], p_max["n"])
            )
    else:
        ratio = adj_max / adj_min
        g["ratio"] = ratio
        g["verdict"] = "ok"
        # The gate is per decade of N, not per span. "constant ~ 1, linear ~ 10"
        # is a statement about 10x more work; measured endpoint-to-endpoint over
        # 100 -> 10000 that same linear term reads as 100x, and a merely
        # log-shaped cost reads as its per-decade growth squared. Comparing the
        # per-decade figure to the bound keeps one threshold meaningful no
        # matter how many points an axis declares.
        decades = math.log10(float(p_max["n"]) / float(p_min["n"])) if p_min["n"] > 0 else 0.0
        if decades > 0:
            g["decades"] = decades
            g["ratio_per_decade"] = ratio ** (1.0 / decades)
            g["conditional_fail"] = g["ratio_per_decade"] > flat_max
        else:
            # Same N at both ends: nothing to slope against, so nothing to gate.
            g["notes"].append("smallest and largest n are equal; no slope to measure")
            g["conditional_fail"] = False

    # Consecutive-point ratios exist so a reader can tell increments-growing
    # (a real linear term) apart from a single noisy endpoint. Reported for
    # every group that reached this point, regardless of verdict.
    cr = []
    for i in range(len(points) - 1):
        a, b = points[i]["adjusted_ms"], points[i + 1]["adjusted_ms"]
        cr.append(b / a if a > 0 else None)
    g["consecutive_ratios"] = cr
    bits = []
    for i, r in enumerate(cr):
        a_n, b_n = points[i]["n"], points[i + 1]["n"]
        bits.append("%d->%d x%.2f" % (a_n, b_n, r) if r is not None else "%d->%d n/a" % (a_n, b_n))
    g["notes"].append("consecutive: " + ", ".join(bits))

    if clamped_at:
        g["notes"].append(
            "negative adjusted mean (faster than floor, within noise) clamped to 0.0 at n="
            + ",".join(str(n) for n in clamped_at)
        )

    g["fails"] = g["always_fail"] or (g["gates"] and g["conditional_fail"])
    return g


def format_point_cell(p):
    e = p["elem"]
    cell = "%.2f±%.2f" % (e["mean_ms"], e["stddev_ms"])
    if "adjusted_ms" in p:
        cell += " (%.2f)" % p["adjusted_ms"]
    return cell


def provenance_lines(meta, flat_max):
    versions = meta.get("versions") or {}
    host = meta.get("host") or {}
    axes = meta.get("axes") or {}

    def shown(x):
        return x if x not in (None, "") else "(not measured)"

    lines = []
    lines.append("# fufu benchmark report")
    lines.append("")
    lines.append(
        "versions: ff=%s git=%s jj=%s hyperfine=%s"
        % (shown(versions.get("ff")), shown(versions.get("git")), shown(versions.get("jj")), shown(versions.get("hyperfine")))
    )
    lines.append(
        "host: os=%s arch=%s cpu=%s nproc=%s kernel=%s"
        % (host.get("os"), host.get("arch"), shown(host.get("cpu")), host.get("nproc"), host.get("kernel"))
    )
    lines.append("ff binary: %s" % meta.get("ff_binary"))
    axis_bits = ["%s=%s" % (name, spec.get("points")) for name, spec in axes.items()]
    lines.append("axes: " + ", ".join(axis_bits))
    lines.append("flat max: %.2f" % flat_max)
    return lines


def axis_table(axis, group_list):
    group_list = list(group_list)
    ns = sorted(set(p["n"] for g in group_list for p in g["points"]))
    # expect is a column, not a footnote: a linear-declared row can sit at 9.8x
    # with verdict "ok", and without it on screen that reads as a passing flat
    # row rather than one the gate was never scoped to.
    header = (
        ["row", "tool", "expect", "command"]
        + ["n=%d" % n for n in ns]
        + ["ratio", "per decade", "verdict", "notes"]
    )

    lines = ["## " + axis, ""]
    lines.append("| " + " | ".join(header) + " |")
    lines.append("|" + "|".join(["---"] * len(header)) + "|")
    for g in group_list:
        by_n = dict((p["n"], p) for p in g["points"])
        cells = [g["row"], g["tool"], g["expect"], "`%s`" % g["command"]]
        for n in ns:
            p = by_n.get(n)
            cells.append(format_point_cell(p) if p else "-")
        cells.append("%.2fx" % g["ratio"] if g["ratio"] is not None else "n/a")
        cells.append(
            "%.2fx" % g["ratio_per_decade"] if g["ratio_per_decade"] is not None else "n/a"
        )
        cells.append(g["verdict"] + (" [FAIL]" if g["fails"] else ""))
        cells.append("; ".join(g["notes"]))
        lines.append("| " + " | ".join(cells) + " |")
    return lines


def fail_block(failing, flat_max):
    if not failing:
        return []
    n = len(failing)
    lines = [
        "FAIL: %d flat row%s scaled with %s axis" % (n, "" if n == 1 else "s", "its" if n == 1 else "their")
    ]
    for g in failing:
        if g["verdict"] == "no-floor":
            missing_ns = [str(p["n"]) for p in g["points"] if p["floor"] is None]
            lines.append(
                "  %s / %s / %s: no floor measured for n=%s"
                % (g["row"], g["axis"], g["tool"], ",".join(missing_ns))
            )
        else:
            p_min, p_max = g["points"][0], g["points"][-1]
            adj_min = p_min.get("adjusted_ms", 0.0)
            adj_max = p_max.get("adjusted_ms", 0.0)
            ratio_str = "%.2f" % g["ratio"] if g["ratio"] is not None else "n/a"
            per_dec = g["ratio_per_decade"]
            per_dec_str = "%.2f" % per_dec if per_dec is not None else "n/a"
            lines.append(
                "  %s / %s / %s: %sx per decade (flat max %.2f; %sx overall) -- %.2fms @%d -> %.2fms @%d"
                % (
                    g["row"],
                    g["axis"],
                    g["tool"],
                    per_dec_str,
                    flat_max,
                    ratio_str,
                    adj_min,
                    p_min["n"],
                    adj_max,
                    p_max["n"],
                )
            )
    return lines


def print_markdown(meta, groups, flat_max, failing, quiet):
    lines = []
    if not quiet:
        lines.extend(provenance_lines(meta, flat_max))
        lines.append("")
        lines.append(FRAMING_LINE)
        lines.append("")
        for axis, axis_groups in itertools.groupby(groups, key=lambda g: g["axis"]):
            lines.extend(axis_table(axis, axis_groups))
            lines.append("")

    lines.extend(fail_block(failing, flat_max))

    if not quiet and not failing:
        gated = [g for g in groups if g["gates"]]
        if gated:
            lines.append("PASS: %d flat ff row(s) within %.2fx" % (len(gated), flat_max))
        else:
            lines.append("PASS: no flat rows measured")

    if not lines:
        return
    print("\n".join(lines).rstrip("\n"))


def point_summary(p):
    e = p["elem"]
    out = {
        "n": p["n"],
        "mean_ms": e["mean_ms"],
        "stddev_ms": e["stddev_ms"],
        "median_ms": e["median_ms"],
        "min_ms": e["min_ms"],
        "max_ms": e["max_ms"],
        "runs": e["runs"],
        "exit_ok": e["exit_ok"],
    }
    if p["floor"] is not None:
        out["floor_mean_ms"] = p["floor"]["mean_ms"]
        out["floor_stddev_ms"] = p["floor"]["stddev_ms"]
    if "adjusted_ms" in p:
        out["adjusted_ms"] = p["adjusted_ms"]
    return out


def group_summary(g):
    return {
        "axis": g["axis"],
        "row": g["row"],
        "tool": g["tool"],
        "expect": g["expect"],
        "command": g["command"],
        "verdict": g["verdict"],
        "ratio": g["ratio"],
        "ratio_per_decade": g["ratio_per_decade"],
        "decades": g["decades"],
        "consecutive_ratios": g["consecutive_ratios"],
        "gates": g["gates"],
        "fails": g["fails"],
        "notes": g["notes"],
        "points": [point_summary(p) for p in g["points"]],
    }


def build_json(meta, groups, flat_max, failing, quiet):
    groups_out = failing if quiet else groups
    return {
        "meta": {
            "generated_unix": meta.get("generated_unix"),
            "host": meta.get("host"),
            "versions": meta.get("versions"),
            "ff_binary": meta.get("ff_binary"),
            "axes": meta.get("axes"),
            "flat_ratio_max_used": flat_max,
        },
        "framing": FRAMING_LINE,
        "groups": [group_summary(g) for g in groups_out],
        "summary": {
            "ok": not failing,
            "failing_count": len(failing),
            "failing": ["%s/%s/%s" % (g["row"], g["axis"], g["tool"]) for g in failing],
        },
    }


def main(argv=None):
    parser = argparse.ArgumentParser(
        prog="report.py",
        description="Gate and render fufu's flat-vs-linear bench results (see bench-schema.md).",
    )
    parser.add_argument("path", nargs="?", default=DEFAULT_PATH, help="raw.json path, or - for stdin")
    parser.add_argument("--flat-max", type=float, default=None, help="override meta.flat_ratio_max (default 1.5)")
    parser.add_argument("--json", action="store_true", help="emit the computed analysis as JSON instead of markdown")
    parser.add_argument("--quiet", action="store_true", help="print only failures")
    args = parser.parse_args(argv)

    try:
        doc = load_doc(args.path)
    except (OSError, ValueError) as e:
        print("error: could not read %s: %s" % (args.path, e), file=sys.stderr)
        return 2

    flat_max = args.flat_max
    if flat_max is None:
        flat_max = doc.get("meta", {}).get("flat_ratio_max", DEFAULT_FLAT_MAX)

    try:
        meta, groups = analyze(doc, flat_max)
    except (KeyError, TypeError, ValueError) as e:
        print("error: malformed bench results: %s" % (e,), file=sys.stderr)
        return 2

    failing = [g for g in groups if g["fails"]]

    if args.json:
        print(json.dumps(build_json(meta, groups, flat_max, failing, args.quiet), indent=2))
    else:
        print_markdown(meta, groups, flat_max, failing, args.quiet)

    return 1 if failing else 0


if __name__ == "__main__":
    sys.exit(main())
