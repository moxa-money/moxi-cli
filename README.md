# moxi-cli

Moxi by Moxa. The install binary is `moxi`.

Founder settings live in `~/.moxi` (`%USERPROFILE%\.moxi` on Windows), in `config.yaml`. Set `MOXI_CONFIG_DIR` to use another directory.

This build registers `config`, `ctx`, `version`, `usage`, `help` (including `help tree`), and `setup` (`doctor`, `cursor`). `ctx` stores a local label. It is not a cloud environment.

`moxi setup doctor` checks the config file, the AWS CLI, GCP CLI, and sign-in for AWS, GCP, and Google Workspace. It stops at the first miss and prints one remediation. It does not print a token or an account id. `moxi setup cursor --enable` writes editor rules and an example MCP file. The example has no servers and no keys.

The open-source license is not chosen yet. See `LICENSE`.

```
go build -o moxi ./cmd/moxi
go test ./...
```

`go install github.com/moxa-money/moxi-cli/cmd/moxi@latest` installs the `moxi` binary once this repo is published.
