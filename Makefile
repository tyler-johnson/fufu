# Daily driver: `make` = fast dogfood build; ~/.cargo/bin/ff symlinks to
# target/dogfood/ff, so the binary is live the moment it links.
# `make release` is the honest fat-LTO build benches and releases use.

.PHONY: build release

build:
	cargo build --profile dogfood

release:
	cargo build --release
