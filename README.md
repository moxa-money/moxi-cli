# moxi-cli

Moxi is a founder shell you run on your own computer. It keeps your local settings, an empty ledger of plans and requests, and a desk you can read in the terminal or as a local page. It also includes an example of a closed cloud network you can plan in an AWS account you own.

You keep the accounts. The repository ships no key, no account id, and no filled ledger.

**Alpha, untested release.** This repository is public for grant review. It is not ready for use. There is no roadmap.

## What you need

- Git, Go 1.24 or newer, and Rust with Cargo, to build `moxi` and the ledger.
- Terraform 1.6 or newer, and the AWS provider 5 or newer, when you want to read the example plan.
- An AWS account you control, and the [AWS CLI](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html). The check asks you to sign in yourself with `aws login`.
- The [Google Cloud CLI](https://cloud.google.com/sdk/docs/install), and a Google Workspace where you are an admin. The check confirms you can read the directory. It stops at the first miss and prints one next step.
- A Mac, only when you choose to approve a change that can cost money. That approval is the Mac biometric prompt.

Settings live in `~/.moxi/config.yaml` (`%USERPROFILE%\.moxi\config.yaml` on Windows). `MOXI_CONFIG_DIR` selects another directory. The person at the desk is `FOUNDER`.

## Paid resources

Reading the example plan does not create cloud resources. Applying it later can. The example can create a NAT gateway, a KMS key, CloudWatch log groups, VPC flow logs, and an empty ECS cluster. AWS bills those. Leave public ingress off unless you have decided otherwise. Do not point this example at a production account. State stays on your machine.

A paid change needs `moxi setup consent` and a signature over that exact plan and cost table. A yes in chat is not an approval. This release cannot complete a paid signature without the Mac biometric.

## Where to go next

| You want | Read |
|---|---|
| To set it up with an agent | [docs/setup/operator.md](docs/setup/operator.md) |
| The example network and how the files connect | [terraform/README.md](terraform/README.md) |
| The ledger | [ledger/README.md](ledger/README.md) |
| The tool list | [tools/catalog.md](tools/catalog.md) |
| The type map | [ontology/mappings.md](ontology/mappings.md) |
| A Mac code-signing identity for a later watcher app | [docs/setup/moxa-watchers.md](docs/setup/moxa-watchers.md) |

Agents follow [AGENTS.md](AGENTS.md). `moxi usage --output json` is the command list. The Moxa desk `moxa` defaults to `{shape, views.json}`; rows are `views.json.rows`; `moxa ui` needs `--output tui`; `moxa strategy get` lists every strategy.

```sh
go build -o moxi ./cmd/moxi
go test ./...
cargo test --manifest-path ledger/Cargo.toml
```

`go install github.com/moxa-money/moxi-cli/cmd/moxi@latest` installs the `moxi` binary.

The license is the MIT License. See `LICENSE`.
