# Setup

Alpha, untested release. This repository is public for grant review. It is not ready for use. There is no roadmap.

An agent uses this file to help one person stand up the founder shell on that person's machine. The person keeps the cloud accounts. This tree ships no key, no account id, and no filled ledger.

The sitting ends when the local config exists, `moxi setup doctor` is clean or stopped on one named check, the ledger is initialized, the desk prints, and `terraform plan` has been read. Do not apply the shell in this release.

## What the agent does

- Run `moxi usage --output json` before inventing a flag. The registered commands are `config`, `ctx`, `version`, `usage`, `help`, `setup`, `comms`, `desk`, and `dash`.
- Pass `--output json` on `moxi`. Pass `--json` on `comms`.
- The Moxa desk `moxa` defaults to `{shape, views.json}`. Rows are `views.json.rows`. `moxa ui` needs `--output tui`. `moxa strategy get` lists every strategy when `--id` is omitted. `moxi` does not wrap `shape` and `views`.
- Run `moxi setup doctor` after each fix. It stops at the first failed check and prints one remediation. Do the remediation, then run it again.
- Keep secrets out of the chat, the ledger, and any commit. That includes account ids, access tokens, `terraform.tfvars`, `terraform.tfstate`, and `ledger/data/events.jsonl`.
- Leave `terraform apply` unrun. A plan is the end of the cloud step in this release.

## What the person does

- Installs the tools below and clones this repository.
- Completes `aws login` and `gcloud auth login` in their own terminal.
- Completes the Google Workspace admin step when doctor asks for it.
- Approves a paid consent on the Mac biometric prompt, when they choose to try consent. A yes in chat is not an approval.
- Edits `terraform.tfvars` for their own names. The agent does not put an account id in that file.

## Tools on the machine

| Tool | Used for |
|---|---|
| Git | Clone this repository. |
| Go | `go build -o moxi ./cmd/moxi` |
| Rust and Cargo | `cargo build --release --manifest-path ledger/Cargo.toml` |
| AWS CLI | Doctor check `aws-cli` and `aws-auth`. Install from the AWS CLI guide. |
| Google Cloud CLI | Doctor check `gcloud` and `gcp-auth`. Install from the Google Cloud SDK guide. |
| Terraform | `terraform init` and `terraform plan` in `terraform/`. |
| A Mac, only for a later watcher app | `docs/setup/moxa-watchers.md`. This release does not build watcher apps. |

Doctor does not install these tools. The Go version line in doctor is the version of the `moxi` process.

## 1. Build

From the clone:

```sh
go build -o moxi ./cmd/moxi
```

Call `./moxi`, or put that binary on `PATH` as `moxi`. The examples below use `moxi`.

Confirm the contract:

```sh
moxi version
moxi usage --output json
moxi help tree
```

## 2. Founder config

The config file is `config.yaml` under `~/.moxi` on macOS and Linux, and under `%USERPROFILE%\.moxi` on Windows. `MOXI_CONFIG_DIR` selects another directory. The file holds a profile name, a context label, and an output mode. It does not hold a cloud account or a credential.

```sh
moxi config init
moxi config path
moxi config show --output json
moxi ctx show --output json
```

`config init` writes the file once. If it already exists, the command refuses. Use `config show`.

Defaults are profile `default`, context `local`, and output `table`. A context is a local label. It is not a cloud environment. A profile or context name is 1 to 32 characters, lowercase letters, digits, and hyphens, and it starts with a letter.

```sh
moxi ctx use local
moxi config set output json
```

`config set` accepts `output` (`json` or `table`), `context`, and `integrations.cursor.enabled` (`true` or `false`). Any other key is refused.

```sh
moxi config profile list
moxi config profile create <name>
moxi config profile use <name>
```

## 3. Doctor

```sh
moxi setup doctor --output json
```

Checks run in this order. The first failure is the only one to fix.

| Check | Pass | When it fails |
|---|---|---|
| `go-runtime` | The process reports its Go version. | This check does not fail. |
| `config-file` | The config path exists. | Run `moxi config init`. |
| `aws-cli` | `aws` is on `PATH`. | Install the AWS CLI: https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html |
| `aws-auth` | `aws sts get-caller-identity` succeeds. | The person runs `aws login` in their terminal. Do not copy the identity into the chat. |
| `gcloud` | `gcloud` is on `PATH`. | Install the Google Cloud CLI: https://cloud.google.com/sdk/docs/install |
| `gcp-auth` | `gcloud auth print-access-token` succeeds. | The person runs `gcloud auth login`. Do not copy the token. |
| `workspace-auth` | The Workspace directory API returns HTTP 200. | The person opens https://admin.google.com as an admin, then runs `gcloud auth login --scopes=openid,https://www.googleapis.com/auth/admin.directory.user.readonly` |

A Workspace refusal names the HTTP status in the detail line. Reachability failures say `could not reach admin.googleapis.com`. Stop on that line. Do not invent a second command.

Run doctor again after the person finishes the one step. Continue only when `"ok": true`.

## 4. Ledger

The ledger root is the `ledger/` directory in this clone. It is a root because it contains `owners.toml` and a `Cargo.toml` whose package name is `comms`. An empty directory is not a root. `comms` refuses `--dir` or `COMMS_DIR` when that test fails. With neither set, `comms` walks upward from the current directory and uses the first root it finds, including a nested `ledger/` directory.

```sh
cargo build --release --manifest-path ledger/Cargo.toml
export MOXI_LEDGER_DIR="$PWD/ledger"
export COMMS_DIR="$PWD/ledger"
export MOXI_COMMS_BIN="$PWD/ledger/target/release/comms"
moxi comms init --dir "$COMMS_DIR"
```

On Windows, set the same three variables to the absolute `ledger` path and the built `comms.exe`. `moxi comms` runs `MOXI_COMMS_BIN` when it is set, otherwise `comms` on `PATH`.

`init` creates `ledger/data/` and an empty `ledger/data/events.jsonl`. Leave that log uncommitted. The human operator in this build is `FOUNDER` and owns no repo. Example workers live in `ledger/owners.toml` (`PLATFORM`, `APP`, `LEAD`). Replace those names with the person's workers when they have chosen them. A worker name is one word, up to 32 characters.

```sh
moxi comms owners --json
moxi comms ready --json
moxi desk --output json
```

`moxi desk` reads `MOXI_LEDGER_DIR`, or `~/.moxi/ledger` when that variable is unset. Set `MOXI_LEDGER_DIR` to this clone's `ledger/` or the desk reads a different directory than `comms`. An empty log prints status `ledger empty`. After `init`, the status is `ledger open`. Workers come from `owners.toml` in that directory.

```sh
moxi dash
```

This writes `dash.html` in the config directory (`~/.moxi/dash.html` by default). `--out` selects another path. The person opens that file locally. The page is the status line, the activity list, the workers, the command cursor, and the desk link.

A signed answer later reads `ledger/signers/founder.md`. That file is created on this machine. It holds a fingerprint, not a private key. Do not commit it.

## 5. Editor scaffold

From the clone, after the person wants an editor file:

```sh
moxi setup cursor --enable
```

`--enable` sets `integrations.cursor.enabled` and writes the scaffold. Without it, a disabled integration refuses: `cursor integration disabled — run: moxi setup cursor --enable`.

The command writes `.cursor/rules/founder.mdc` and `.cursor/mcp.json.example` when those files are absent. Existing files are left in place. The example MCP file has no servers and no keys. Copy it to `.cursor/mcp.json` only when a later check names a server.

`moxi setup cursor --dir <path>` writes the scaffold in another directory. The default is the current directory.

## 6. Shell plan

The shell is identity, a network that is closed by default, logs, encryption, and an empty place to run an app. State stays on this machine. The example values are in `terraform/terraform.tfvars.example`.

```sh
cd terraform
cp terraform.tfvars.example terraform.tfvars
```

The person may change `name_prefix`, `environment`, `region`, `cidr_block`, `az_count`, `enable_nat`, `enable_flow_logs`, `flow_log_retention_days`, and `tags`. Leave `enable_public_ingress` false unless they have decided otherwise. Do not put an account id or a secret in `tags`. Do not commit `terraform.tfvars` or any `terraform.tfstate`.

```sh
terraform init
terraform plan -var-file=terraform.tfvars
```

Read the plan with the person. Stop there. `terraform apply` is outside this release.

## 7. Consent

Consent is the digest of a plan summary file and a cost table file. Use it before any later paid apply. Keep both files out of git.

```sh
moxi setup consent --plan plan.txt --cost cost.txt --output json
```

The JSON has `digest`, `incurs_cost`, `status` (`canonical`), and `bytes`. An empty cost table, or a table with no amounts, is treated as paid. The words `none`, `no cost`, and `no charge`, or an explicit zero, are free.

```sh
moxi setup consent --plan plan.txt --cost cost.txt --approve
```

A free table stores an approval with method `none`. A paid table asks the person to approve on this Mac with the biometric prompt. The command prints `digest`, `method`, and `status`. It does not print the signature.

If the command returns `biometrics unavailable` or `paid apply refused`, stop. This release has no working WebAuthn fallback, so a paid approval cannot be completed on a machine without the Mac biometric.

A second use of the same signed text is refused. Changing the plan or the cost table changes the digest and needs a new approval.

## 8. Mac signing identity

Skip this step unless the person is about to sign a watcher app. This repository does not build watcher apps.

The person creates a code-signing certificate named `Moxa Watchers` once in Keychain Access. The steps are `docs/setup/moxa-watchers.md`. Setup does not create the certificate and does not know the login password. Do not export the private key. Do not write a `.p12` or a PEM.

Confirm with:

```sh
security find-identity -v -p codesigning
```

The list includes `Moxa Watchers`. That is the identity.

## Done

The sitting is complete when all of these are true:

- `moxi config show --output json` shows the config directory, profile, context, and output.
- `moxi setup doctor --output json` has `"ok": true`, or it is stopped on one check the person has not finished.
- `moxi comms ready --json` runs against `ledger/`.
- `moxi desk --output json` reads that same ledger.
- `terraform plan` was read, and `terraform apply` was not run.
- No account id, token, state file, or filled log was written into the repository or the chat.

`ontology/mappings.md` is the type and predicate map for this shell and the ledger. `tools/catalog.md` is the tool list. Neither file is a second setup path.
