# Tools

An agent walking a person through setup follows `docs/setup/operator.md`.

Each row is a local tool. A credential stays on the founder's machine. This catalog has no key, no account id, and no filled vault.

| Tool | Kind | Command or place | What it is for |
|---|---|---|---|
| Ledger | CLI | `comms`, or `moxi comms` | Plans, needs, and who was told. Empty until `comms init`. |
| Desk | CLI | `moxi desk` | Status line, activity list, command cursor, desk link. |
| Dash | CLI | `moxi dash` | The same desk as a local page. |
| Setup checks | CLI | `moxi setup doctor` | This machine, and sign-in for AWS, GCP, and Google Workspace. |
| Editor scaffold | CLI | `moxi setup cursor --enable` | Editor rules and an example MCP file with no servers and no keys. |
| Paid-apply consent | CLI | `moxi setup consent` | Digest of the plan summary and the cost table. |
| Shell | Terraform | `terraform/` | Identity, closed network, logs, encryption, and an empty place to run an app. |
| Mappings | Doc | `ontology/mappings.md` | Types and predicates for that shell and the ledger. |
| Local memory | Optional process | MuninnDB on `127.0.0.1` | Recall on this machine. It is not a second ledger. The vault starts empty. |

A watcher is a worker named in `ledger/owners.toml`. On a Mac, the code-signing identity for a watcher app is `docs/setup/moxa-watchers.md`. Setup does not create that certificate.
