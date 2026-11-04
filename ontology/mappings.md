# Ontological mappings

Types and predicates for a founder shell. A name in the graph is the founder's. This file does not name a cloud account, a person, or a filled ledger.

Status on a node:

| Code | Means |
|---|---|
| D | Deployed in the founder's account |
| C | Concept only |
| P | Planned, not fully live |

## Types

| type | What it is | Where it lands |
|---|---|---|
| repository | One git root | the founder's repo |
| bounded_context | One product slice | a ledger plan |
| lambda_function | One function | a later apply, not the first sitting |
| ecs_service | A long-running app | `terraform` runtime cluster, then a service the founder adds |
| step_function | A multi-step workflow | a later apply |
| code_location | A path in a repo | the repo |
| deployment_stage | dev, staging, or prod as a label | `environment` in `terraform.tfvars` |
| cloud_resource | A network, bucket, or log the shell creates | `terraform/modules/network` and `logging` |
| iam_role | A role the app assumes | `terraform/modules/identity` |
| kms_key | The shell key | `terraform/modules/encryption` |
| queue | One queue | a later apply |
| database_schema | A logical schema | not in the first sitting |
| api_endpoint | `METHOD /path` | the founder's app |
| sensitivity_tier | pii, financial, or credentials | a tag the founder sets |
| git_branch | `{repo}@main` | the repo |
| pull_request | `{repo}#{n}` | the repo |
| identity | A person, named by the founder | `owners.toml` and the operator `FOUNDER` |
| plan | Ledger plan | `comms plan open` |
| need | Work one worker must do | `comms` need |
| tell | A fact | `comms` tell |
| notice | A board post | `comms notice` |

## Predicates

| rel_type | From | To |
|---|---|---|
| owned_by | lambda_function, ecs_service | bounded_context |
| deployed_in | lambda_function, ecs_service | cloud_resource |
| exists_in_stage | cloud_resource, ecs_service | deployment_stage |
| assumes | ecs_service | iam_role |
| encrypted_by | cloud_resource | kms_key |
| logged_in | cloud_resource | cloud_resource (`#log-group`) |
| assigned_to | plan, pull_request | identity |
| blocks | plan | plan |
| branch_of | git_branch | repository |

The first sitting creates the `cloud_resource`, `iam_role`, `kms_key`, and empty runtime nodes. It does not create a `database_schema`.
