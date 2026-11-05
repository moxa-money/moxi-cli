# Shell

The first sitting is five controls: identity, a network that is closed by default, logs, encryption, and a place to run an app.

The root module links those files by passing an output into the next module. Nothing in this tree reads another account, a remote state file, or a private configuration. Values start in `terraform.tfvars.example`.

| From | Output | Into |
|---|---|---|
| `modules/encryption` | `key_arn` | `modules/logging` `kms_key_arn`, and `modules/network` `kms_key_arn` |
| `modules/logging` | `log_group_arn` | `modules/identity` `log_group_arn` |
| `modules/network` | `private_subnet_ids` | `modules/runtime` `private_subnet_ids` |
| `modules/identity` | `task_role_arn` | `modules/runtime` `task_role_arn` |
| `modules/network` | `app_security_group_id` | Published for a later service. This sitting does not pass it. |
| `modules/identity` | `execution_role_arn` | The role that writes to the shell log group. This sitting does not pass it into the runtime. |
| `ledger/` | `owners.toml`, empty `data/events.jsonl` | The desk, when `MOXI_LEDGER_DIR` points here. |
| `ontology/mappings.md` | `encrypted_by`, `logged_in`, `assumes`, `deployed_in` | The same wires as the rows above. |
| `docs/setup/operator.md` | The sitting | Stops before a paid cloud change. |

Copy `terraform.tfvars.example` to `terraform.tfvars` and change the values. The example file is the configured starting point. It has no account id and no backend. State stays on this machine until the founder chooses a backend in their own account.

Customer data stores are not in this sitting.

```sh
cd terraform
cp terraform.tfvars.example terraform.tfvars
terraform init
terraform plan -var-file=terraform.tfvars
```

Apply stays a separate step. `moxi setup consent` canonicalizes the plan summary and the cost table before a paid apply.
