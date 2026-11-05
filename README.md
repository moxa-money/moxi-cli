# moxi-cli

Alpha, untested release. This repository is public for grant review. It is not ready for use.

Agents helping a person set this up follow [docs/setup/operator.md](docs/setup/operator.md).

The commands below are the Alpha surface. This repository has no roadmap.

Moxi by Moxa. The install binary is `moxi`.

Founder settings live in `~/.moxi` (`%USERPROFILE%\.moxi` on Windows), in `config.yaml`. Set `MOXI_CONFIG_DIR` to use another directory.

This build registers `config`, `ctx`, `version`, `usage`, `help` (including `help tree`), `setup` (`doctor`, `cursor`, `consent`), `comms`, `desk`, and `dash`. `ctx` stores a local label. It is not a cloud environment.

`comms` is the ledger in `ledger/`. It starts empty. Workers are named in `ledger/owners.toml`. The human at the desk is `FOUNDER` in this build and owns no repo. `moxi desk` prints the status line, the activity list, and the command cursor. `moxi dash` writes that same view to a local page.

`ontology/mappings.md` is the type and predicate map for the shell and the ledger. `terraform/` is the generic shell: identity, a closed network, logs, encryption, and an empty place to run an app. Values start in `terraform/terraform.tfvars.example`. `tools/catalog.md` lists those surfaces. None of them ship a key or an account id.

`moxi setup doctor` checks the config file, the AWS CLI, GCP CLI, and sign-in for AWS, GCP, and Google Workspace. It stops at the first miss and prints one remediation. It does not print a token or an account id. `moxi setup cursor --enable` writes editor rules and an example MCP file. The example has no servers and no keys. `moxi setup consent` prints the digest of a plan summary and a cost table.

On a Mac, create the `Moxa Watchers` code-signing identity once in Keychain Access before a watcher app is signed. The steps are `docs/setup/moxa-watchers.md`. Setup does not create that certificate.

The open-source license is the MIT License. See `LICENSE`.

```
go build -o moxi ./cmd/moxi
go test ./...
cargo test --manifest-path ledger/Cargo.toml
```

`go install github.com/moxa-money/moxi-cli/cmd/moxi@latest` installs the `moxi` binary once this repo is published.
