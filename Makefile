BIN := $(HOME)/.cargo/bin/pulse

install:
	cargo build --release
	cp target/release/pulse $(BIN)
	codesign --force --sign - $(BIN)
	pulse reload

.PHONY: install
