# moxi-cli

Moxi by Moxa. The install binary is `moxi`.

Founder settings live in `~/.moxi` (`%USERPROFILE%\.moxi` on Windows), in `config.yaml`. Set `MOXI_CONFIG_DIR` to use another directory.

This build registers `config`, `ctx`, `version`, `usage`, and `help` (including `help tree`). `ctx` stores a local label. It is not a cloud environment.

The open-source license is not chosen yet. See `LICENSE`.

```
go build -o moxi ./cmd/moxi
go test ./...
```

`go install github.com/moxa-money/moxi-cli/cmd/moxi@latest` installs the `moxi` binary once this repo is published.
