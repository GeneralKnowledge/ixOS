.PHONY: test fmt clippy bindings check-bindings doctor package-ios validate

test:
	cargo test --workspace

fmt:
	cargo fmt --all

clippy:
	cargo clippy --workspace --all-targets -- -D warnings

bindings:
	./scripts/generate-bindings.sh

check-bindings:
	./scripts/check-bindings.sh

doctor:
	./scripts/validate-toolchain.sh

package-ios:
	./scripts/package-ios-library.sh

validate: fmt clippy test check-bindings
