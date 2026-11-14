# Agents

This tree is an Alpha, untested release. It is public for grant review. It is not ready for use.

When a person asks for help setting it up, follow `docs/setup/operator.md` from the first incomplete step. Pass `--output json` on `moxi` and `--json` on `comms`. `moxi usage --output json` is the command contract.

The Moxa desk binary `moxa` prints JSON by default. That object is `shape` and `views`. Rows are `views.json.rows`. `moxa ui` draws with `--output tui`. `moxa strategy get` with no `--id` lists every strategy. `work` and `agents list` are shape `list`. This `moxi` binary still prints its own object when `--output json` is set.

The person signs in to AWS, Google Cloud, and Google Workspace in their own terminal. The person approves a paid step with the biometric prompt. A yes in chat is not an approval.

Do not run `terraform apply`. Do not write an account id, a token, a state file, or a filled ledger into the repo or the chat. Do not export the Mac code-signing key.
