CLIPPY_ARGS=-Dwarnings -Drust-2018-idioms -Drust-2021-compatibility -Adeprecated

.PHONY: clippy
clippy:
	cargo clippy --fix --allow-dirty --all --all-targets --all-features -- $(CLIPPY_ARGS)

.PHONY: test
test:
	cargo test --all