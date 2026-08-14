# Daily driver: `make` = fast dogfood build; ~/.cargo/bin/ff symlinks to
# target/dogfood/ff, so the binary is live the moment it links.
# `make release` is the honest fat-LTO build benches and releases use.

.PHONY: build release bench bench-real bench-report bench-against

build:
	cargo build --profile dogfood

release:
	cargo build --release

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

# Compare the working tree against a rebuilt older binary, measured back to
# back on the same fixtures. REF defaults to the most recent tag. Costs a
# full rebuild of the ref (~2m50s cold on a Pi 5, faster once target/against
# is warm) plus two measurement passes. This is a comparison view, not a gate.
bench-against: release
	scripts/bench/against.sh $(or $(REF),$(shell git describe --tags --abbrev=0))
