# Shell

The first sitting is five controls: identity, a network that is closed by default, logs, encryption, and a place to run an app.

Copy `terraform.tfvars.example` to `terraform.tfvars` and change the values. The example file is the configured starting point. It has no account id and no backend. State stays on this machine until the founder chooses a backend in their own account.

Customer data stores are not in this sitting.

```sh
cd terraform
cp terraform.tfvars.example terraform.tfvars
terraform init
terraform plan -var-file=terraform.tfvars
```

Apply stays a separate step. `moxi setup consent` canonicalizes the plan summary and the cost table before a paid apply.
