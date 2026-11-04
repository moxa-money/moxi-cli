# Ledger

Founder-local coordination ledger. The binary is `comms`. The log is `data/events.jsonl`. SQLite is a projection compiled into the binary. It starts empty.

Workers are the names in `owners.toml`. The human operator in this build is `FOUNDER` and owns no repo. `FOUNDER` is the name this build uses for the person at the desk. A signed answer checks `signers/founder.md` in this directory. That file is not in git.

```sh
cargo build --release --manifest-path ledger/Cargo.toml
export MOXI_LEDGER_DIR="$PWD/ledger"
./ledger/target/release/comms init --dir "$MOXI_LEDGER_DIR"
./ledger/target/release/comms ready --dir "$MOXI_LEDGER_DIR" --json
```

`moxi comms` runs that binary when it is on `PATH` or when `MOXI_COMMS_BIN` points at it. `MOXI_LEDGER_DIR` is the ledger root. `COMMS_DIR` is the same path for the `comms` binary.

This tree does not ship a filled log, a hosted table, or a cloud account.
