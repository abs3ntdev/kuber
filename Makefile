PREFIX ?= $(HOME)/.local

.PHONY: build install uninstall clean

build:
	cargo build --release

install: build
	install -d $(PREFIX)/bin
	install -m 755 target/release/kuber $(PREFIX)/bin/kuber

uninstall:
	rm -f $(PREFIX)/bin/kuber

clean:
	cargo clean
