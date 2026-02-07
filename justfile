default:
    @just --list

build:
    cargo build -q --release

debug:
    cargo build -q

install: build
    cp target/release/tgcli /opt/homebrew/bin/tgcli

clean:
    cargo clean

check:
    cargo check

test:
    cargo test

fmt:
    cargo fmt

clippy:
    cargo clippy -- -W clippy::all
