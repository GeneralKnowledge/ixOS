.PHONY: mock-chat test check check-template test-android-host test-swift-host fmt clippy bindings bindings-kotlin check-bindings doctor package-ios package-android validate

test:
	cargo test --workspace

# In-memory chat server for the Chat example (http://127.0.0.1:8787).
# Pass options with ARGS, e.g. make mock-chat ARGS="--bind 0.0.0.0:8787".
mock-chat:
	cargo run -q -p mock-chat-server -- $(ARGS)

# Full local CI matrix (fmt, clippy, tests, binding drift, template sync, host tests).
check:
	cargo run -q -p forge-cli -- check

test-android-host:
	cargo build -p forge-ffi
	cd examples/counter-android && ./gradlew :app:testDebugUnitTest

test-swift-host:
	./scripts/test-swift-host.sh examples/counter
	./scripts/test-swift-host.sh examples/weather

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

bindings:
	./scripts/generate-bindings.sh examples/counter/Generated
	./scripts/generate-bindings.sh examples/weather/Generated

bindings-kotlin:
	./scripts/generate-bindings.sh examples/counter-android/Generated kotlin

check-bindings:
	./scripts/check-bindings.sh

check-template:
	./scripts/check-template-sync.sh

doctor:
	./scripts/validate-toolchain.sh

package-ios:
	./scripts/package-ios-library.sh

package-android:
	./scripts/package-android-library.sh

validate: fmt clippy test check-bindings check-template
