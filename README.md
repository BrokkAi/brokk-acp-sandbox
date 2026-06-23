# brokk-acp-sandbox

Pure-parsing logic shared between the native [`brokk-anvil`](https://github.com/BrokkAi/anvil)
binary and a `wasm32-wasip2` sandboxed counterpart hosted in-process by
[wasmtime](https://wasmtime.dev/).

The crate ships two things:

1. **A Rust library** with parsers for SKILL.md YAML frontmatter, recursive
   file-content search (using the linear-time `regex` engine), a typed
   shell-like workspace toolbox, and a minimal read-only zip reader. Linked
   into the host for the native fast path.
2. **A pre-built `.wasm` binary** (`brokk_acp_sandbox::WASM_BYTES`) exposing
   the same parsers over JSON-RPC on stdin/stdout. The host loads these bytes
   into wasmtime to parse untrusted input with memory/CPU isolation.

## Typed workspace toolbox

The toolbox extends the guest beyond pure parsing without introducing raw
shell evaluation. Hosts call `run_workspace_command` in the library or send
the `runWorkspaceCommand` JSON-RPC method with a tagged `WorkspaceCommand`
payload.

Supported command variants in this initial API:

- `pwd`
- `ls`
- `cat`
- `head`
- `tail`
- `wc`
- `find`
- `grep`
- `mkdir`
- `cp`
- `mv`
- `rm`

The model is intentionally small and explicit:

- there is no raw shell text, so quoting and tokenization are not part of the API
- pipelines, redirects, `&&`, `||`, `;`, env-prefix assignments, subshells, and command substitution are unsupported
- all paths are workspace-relative and validated to stay under the preopened root
- `grep` reuses the existing bounded recursive search engine
- `cp` rejects directory copies unless `recursive` is true
- recursive mutation paths stay bounded through explicit entry and byte caps

Example JSON-RPC request:

```json
{
  "id": 7,
  "method": "runWorkspaceCommand",
  "params": {
    "guestRoot": "/workspace",
    "command": {
      "command": "head",
      "path": "src/main.rs",
      "lines": 20,
      "maxBytes": 65536
    }
  }
}
```

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

CI verifies that the sandbox still builds for `wasm32-wasip2` and that both the
fresh build and committed `.wasm` expose the expected JSON-RPC entry points.

## License

LGPL-3.0-only. See `LICENSE` and `LICENSE.GPL`.
