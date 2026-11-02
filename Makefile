# The npm package @pflow-xyz/pflow-wasm. Everything else here is plain cargo.
.PHONY: wasm-npm wasm-npm-test wasm-npm-publish wasm-mcp-engine wasm-mcp wasm-mcp-test wasm-mcp-crosscheck wasm-mcp-publish

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

# @pflow-xyz/pflow-mcp: the wasm engine as a stdio MCP server, `npx`-able.
# It ships two builds of the engine: the browser package's (embedded in the
# pages artifact_html writes, so they stay small) and one with the `hosted`
# feature (the pflow.xyz/mcp tools, crates/pflow-hosted), which the server runs.
# The hosted build has its own target dir so the two do not rebuild each other.
MCP_OUT := target/npm/pflow-mcp
MCP_TARGET_DIR := target/wasm-mcp-build
MCP_WASM_BUILD := $(MCP_TARGET_DIR)/wasm32-unknown-unknown/wasm-release/pflow_wasm.wasm
MCP_BINDGEN_OUT := target/wasm-bindgen-mcp
MCP_ENGINE_OUT := target/npm/pflow-wasm-hosted
wasm-mcp-engine:
	@wasm-bindgen --version | grep -qx 'wasm-bindgen $(WASM_BINDGEN_VERSION)' || { \
	  echo 'need: cargo install wasm-bindgen-cli --version $(WASM_BINDGEN_VERSION) --locked'; exit 1; }
	cargo build --profile wasm-release -p pflow-wasm --features hosted --target wasm32-unknown-unknown --target-dir $(MCP_TARGET_DIR)
	wasm-bindgen --target web --out-dir $(MCP_BINDGEN_OUT) --out-name pflow-wasm $(MCP_WASM_BUILD)
	npx --yes --package=binaryen@$(BINARYEN_VERSION) -- wasm-opt -O1 $(WASM_FEATURES) \
	  --strip-debug --strip-producers \
	  -o $(MCP_BINDGEN_OUT)/pflow-wasm_bg.opt.wasm $(MCP_BINDGEN_OUT)/pflow-wasm_bg.wasm
	node crates/pflow-wasm/npm/build.mjs $(MCP_BINDGEN_OUT) $(MCP_BINDGEN_OUT)/pflow-wasm_bg.opt.wasm $(MCP_ENGINE_OUT)

wasm-mcp: wasm-npm wasm-mcp-engine
	node crates/pflow-wasm/mcp/build.mjs $(NPM_OUT) $(MCP_OUT) $(MCP_ENGINE_OUT)
	cd $(MCP_OUT) && npm install --omit=dev --no-audit --no-fund --package-lock=false

wasm-mcp-test: wasm-mcp
	cp crates/pflow-wasm/mcp/test.mjs $(MCP_OUT)/test.mjs   # resolves the SDK from the package's node_modules
	node $(MCP_OUT)/test.mjs $(MCP_OUT); rc=$$?; rm -f $(MCP_OUT)/test.mjs; exit $$rc
	cd $(MCP_OUT) && npm pack --dry-run 2>&1 | grep -E 'total files|package size|unpacked'

# Network: the built server's `reachability` against pflow.xyz/mcp's
# pflow_reachability on tests/reachability/nets. The offline guard is
# `cargo test -p pflow-wasm --test reachability_parity`.
wasm-mcp-crosscheck: wasm-mcp
	cp crates/pflow-wasm/mcp/crosscheck-hosted.mjs $(MCP_OUT)/crosscheck.mjs
	node $(MCP_OUT)/crosscheck.mjs $(MCP_OUT) crates/pflow-wasm/tests/reachability/nets; rc=$$?; rm -f $(MCP_OUT)/crosscheck.mjs; exit $$rc

# Needs `npm login` as an owner of the @pflow-xyz scope. The package has its own
# version (crates/pflow-wasm/mcp/build.mjs), and versions cannot be republished.
wasm-mcp-publish: wasm-mcp-test
	cd $(MCP_OUT) && npm publish --access public
