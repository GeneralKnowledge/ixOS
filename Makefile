.PHONY: check-app-crates chat-server test check check-template check-fresh-app test-android-host test-swift-host fmt clippy bindings bindings-kotlin check-bindings doctor package-ios package-android validate

test:
	cargo test --workspace

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
	cargo build -p forge-ffi -p chat-server
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

bindings:
	./scripts/generate-bindings.sh examples/counter/Generated
	./scripts/generate-bindings.sh examples/weather/Generated
	FORGE_APP_DIR=examples/chat ./scripts/generate-bindings.sh examples/chat/Generated

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

package-ios:
	./scripts/package-ios-library.sh

package-android:
	./scripts/package-android-library.sh

validate: fmt clippy test check-bindings check-template
