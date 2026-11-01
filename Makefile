# The npm package @pflow-xyz/pflow-wasm. Everything else here is plain cargo.
.PHONY: wasm-npm wasm-npm-test wasm-npm-publish

# Must equal the wasm-bindgen dependency in crates/pflow-wasm/Cargo.toml.
WASM_BINDGEN_VERSION := 0.2.129
BINARYEN_VERSION := 132.0.0
WASM_BUILD := target/wasm32-unknown-unknown/wasm-release/pflow_wasm.wasm
WASM_BINDGEN_OUT := target/wasm-bindgen
NPM_OUT := target/npm/pflow-wasm
# What rustc's wasm32-unknown-unknown target emits by default; wasm-opt refuses
# a module that uses a feature it was not told about.
WASM_FEATURES := --enable-bulk-memory --enable-mutable-globals --enable-nontrapping-float-to-int \
	--enable-reference-types --enable-sign-ext --enable-multivalue

# -O1, not -Oz: what a page downloads is the gzipped size, and on this module
# -Oz is 7% smaller raw but 10% larger gzipped (161.8 KB at -O1, 178.7 KB at -Oz).
# Re-measure before changing it.
#
# Builds target/npm/pflow-wasm/. Needs the wasm32-unknown-unknown target,
# `cargo install wasm-bindgen-cli --version $(WASM_BINDGEN_VERSION) --locked`, and node.
wasm-npm:
	@wasm-bindgen --version | grep -qx 'wasm-bindgen $(WASM_BINDGEN_VERSION)' || { \
	  echo 'need: cargo install wasm-bindgen-cli --version $(WASM_BINDGEN_VERSION) --locked'; exit 1; }
	cargo build --profile wasm-release -p pflow-wasm --target wasm32-unknown-unknown
	wasm-bindgen --target web --out-dir $(WASM_BINDGEN_OUT) --out-name pflow-wasm $(WASM_BUILD)
	npx --yes --package=binaryen@$(BINARYEN_VERSION) -- wasm-opt -O1 $(WASM_FEATURES) \
	  --strip-debug --strip-producers \
	  -o $(WASM_BINDGEN_OUT)/pflow-wasm_bg.opt.wasm $(WASM_BINDGEN_OUT)/pflow-wasm_bg.wasm
	node crates/pflow-wasm/npm/build.mjs $(WASM_BINDGEN_OUT) $(WASM_BINDGEN_OUT)/pflow-wasm_bg.opt.wasm $(NPM_OUT)

# The built package: each entry, no-fetch for the inline one, the size budget,
# the byte-exact SSA goldens through the optimized module, the README's snippet,
# and a real browser under a sandbox's CSP (skipped if there is no Chrome).
wasm-npm-test: wasm-npm
	node crates/pflow-wasm/npm/test.mjs $(NPM_OUT)
	node crates/pflow-wasm/npm/test-csp.mjs $(NPM_OUT)
	cd $(NPM_OUT) && npm pack --dry-run

# Needs `npm login` as an owner of the @pflow-xyz scope. Versions cannot be
# republished: bump [workspace.package] version in Cargo.toml first.
wasm-npm-publish: wasm-npm-test
	cd $(NPM_OUT) && npm publish --access public
