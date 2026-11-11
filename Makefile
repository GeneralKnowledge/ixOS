.PHONY: check-app-crates chat-server test check check-template check-fresh-app test-android-host test-swift-host fmt clippy bindings bindings-kotlin check-bindings doctor package-ios package-android validate

test:
	cargo test --workspace
	cargo test -p forge-services --features transfers,http

# The Chat example's server (http://127.0.0.1:8787; accounts and messages in
# ~/.local/share/ixos-chat/server.json). Pass options with ARGS, e.g.
# make chat-server ARGS="--bind 0.0.0.0:8787" to reach it from a phone.
chat-server:
	cargo run -q -p chat-server -- $(ARGS)

# Talk to that server from a terminal: make chat-cli ARGS="send general hi".
chat-cli:
	cargo run -q -p chat-cli -- $(ARGS)

# Full local CI matrix (fmt, clippy, tests, binding drift, template sync, host tests).
check:
	cargo run -q -p forge-cli -- check

test-android-host:
	cargo build --manifest-path examples/counter/rust/Cargo.toml
	cargo build --manifest-path examples/chat-android/rust/Cargo.toml
	cargo build -p chat-server
	cd examples/counter-android && ./gradlew :app:testDebugUnitTest
	cd examples/chat-android && ./gradlew :app:testDebugUnitTest

test-swift-host:
	./scripts/test-swift-host.sh examples/counter
	./scripts/test-swift-host.sh examples/weather
	./scripts/test-swift-host.sh examples/chat
	./scripts/test-swift-host.sh examples/pocket-library

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings
	cargo clippy -p forge-services --all-targets --features transfers,http -- -D warnings

bindings:
	./scripts/generate-bindings.sh examples/counter/Generated
	./scripts/generate-bindings.sh examples/weather/Generated
	./scripts/generate-bindings.sh examples/chat/Generated
	./scripts/generate-bindings.sh examples/pocket-library/Generated

bindings-kotlin:
	./scripts/generate-bindings.sh examples/counter-android/Generated kotlin
	./scripts/generate-bindings.sh examples/chat-android/Generated kotlin

check-bindings:
	./scripts/check-bindings.sh

# Apps from forge init in examples/ (own Cargo workspace): fmt, clippy, tests, bindings.
check-app-crates:
	./scripts/check-app-crates.sh

check-template:
	./scripts/check-template-sync.sh

# forge init --path outside the checkout, then the app's own crate and bindings.
check-fresh-app:
	./scripts/check-fresh-app.sh

doctor:
	./scripts/validate-toolchain.sh

# Package one app's Rust library and bindings: make package-ios APP=examples/chat
package-ios:
	./scripts/package-ios-library.sh $(or $(APP),examples/counter)/Generated

package-android:
	./scripts/package-android-library.sh $(or $(APP),examples/counter-android)/Generated

validate: fmt clippy test check-bindings check-template
