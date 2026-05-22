# brokk-acp-sandbox

Pure-parsing logic shared between the native [`brokk-anvil`](https://github.com/BrokkAi/anvil)
binary and a `wasm32-wasip2` sandboxed counterpart hosted in-process by
[wasmtime](https://wasmtime.dev/).

The crate ships two things:

1. **A Rust library** with parsers for SKILL.md YAML frontmatter, recursive
   file-content search (using the linear-time `regex` engine), and a minimal
   read-only zip reader. Linked into the host for the native fast path.
2. **A pre-built `.wasm` binary** (`brokk_acp_sandbox::WASM_BYTES`) exposing
   the same parsers over JSON-RPC on stdin/stdout. The host loads these bytes
   into wasmtime to parse untrusted input with memory/CPU isolation.

## Why a pre-built `.wasm` is checked in

The `.wasm` is committed in `wasm/brokk-acp-sandbox.wasm` and shipped in the
published tarball as `brokk_acp_sandbox::WASM_BYTES`. Consumers do not need
to install the wasm toolchain to build against this crate.

To rebuild after changing parser code:

```bash
rustup target add wasm32-wasip2
cargo build --release --bin brokk-acp-sandbox --target wasm32-wasip2
cp target/wasm32-wasip2/release/brokk-acp-sandbox.wasm wasm/brokk-acp-sandbox.wasm
```

CI verifies the committed `.wasm` matches a fresh build so drift between the
native library half and the sandboxed binary half cannot land unnoticed.

## License

LGPL-3.0-only. See `LICENSE` and `LICENSE.GPL`.
