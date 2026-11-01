.PHONY: test check-template test-android-host test-swift-host fmt clippy bindings bindings-kotlin check-bindings doctor package-ios package-android validate

test:
	cargo test --workspace

test-android-host:
	cargo build -p forge-ffi
	cd examples/counter-android && ./gradlew :app:testDebugUnitTest

test-swift-host:
	./scripts/test-swift-host.sh

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

bindings:
	./scripts/generate-bindings.sh

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
