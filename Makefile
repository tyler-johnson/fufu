# Daily driver: `make` = fast dogfood build; ~/.cargo/bin/ff symlinks to
# target/dogfood/ff, so the binary is live the moment it links.
# `make release` is the honest fat-LTO build benches and releases use.

.PHONY: build release test fmt fmt-check lint install clean bench bench-real bench-report bench-against bench-docs docs docs-serve docs-gen demo demo-check

build:
	cargo build --profile dogfood

release:
	cargo build --release

test:
	cargo test --workspace

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all --check

lint:
	cargo clippy --workspace --all-targets -- -D warnings

# Point ~/.cargo/bin/ff at the dogfood binary. That is the whole install:
# the symlink is what makes `make` live. Idempotent; rerun after a move.
install: build
	@mkdir -p $(HOME)/.cargo/bin
	ln -sfn $(CURDIR)/target/dogfood/ff $(HOME)/.cargo/bin/ff
	@echo "linked $(HOME)/.cargo/bin/ff -> $(CURDIR)/target/dogfood/ff"

clean:
	cargo clean

# The full local matrix: both axes, default points (100/1000/10000), against
# target/release/ff -- never target/dogfood/ff (see line 1, Cargo.toml's
# [profile.dogfood] comment). report.py's exit status is what make bench
# reports, so a scaled row fails the build.
bench: release
	scripts/bench/run.sh
	scripts/bench/report.py

# The same commands against a real public repository -- git/git by default,
# `make bench-real REPO=linux` for the kernel. Reported, never gated: it
# clones over the network, so it is no one's build gate, and its job is to
# show the hermetic axes' claim holding somewhere a reader can check.
bench-real: release
	scripts/bench/run.sh --axis real-history --real-repo $(or $(REPO),git) --out bench-results/real.json
	scripts/bench/report.py bench-results/real.json

# Re-analyzing an existing raw.json is instant; re-measuring is minutes, so
# this skips run.sh entirely.
bench-report:
	scripts/bench/report.py

# Rewrites the tables on docs/performance.md from the last measurement. The
# numbers are this machine's, so this is a release-time step, not a gate.
bench-docs:
	scripts/bench/docs-table.py

# The docs site. mkdocs comes from docs/requirements.txt (pip install -r);
# docs-gen regenerates everything docsgen.rs owns — the CLI reference from
# the help pages, the config registry region — the same walks CI runs as
# drift checks.
# asset-paths.py first: mkdocs --strict checks markdown links and says
# nothing about a src= in raw HTML, which is how a 404 shipped once.
docs:
	scripts/docs/asset-paths.py
	mkdocs build --strict

docs-serve:
	mkdocs serve

docs-gen:
	FF_DOCS_GEN=1 cargo test -p ff-cli --bins docsgen

# The recordings: the demo on the README and the docs home page, and one per
# tutorial section. Rendering runs the real binary in a real terminal, so it
# needs vhs, ttyd, ffmpeg, a headless chromium and JetBrains Mono on the
# machine; the checks need none of them, which is why CI runs the checks and
# a human runs the render. A failing check is the signal that a recording is
# stale.
demo:
	vhs scripts/docs/demo.tape
	scripts/docs/tutorial-tapes.sh

demo-check:
	scripts/docs/demo-check.sh
	scripts/docs/tutorial-tapes.sh --check

# Compare the working tree against a rebuilt older binary, measured back to
# back on the same fixtures. REF defaults to the most recent tag. Costs a
# full rebuild of the ref (~2m50s cold on a Pi 5, faster once target/against
# is warm) plus two measurement passes. This is a comparison view, not a gate.
bench-against: release
	scripts/bench/against.sh $(or $(REF),$(shell git describe --tags --abbrev=0))
