output "vpc_id" {
  description = "VPC for the closed shell."
  value       = module.network.vpc_id
}

output "private_subnet_ids" {
  description = "Subnets where an app can run. They have no inbound route from the internet when enable_public_ingress is false."
  value       = module.network.private_subnet_ids
}

output "kms_key_arn" {
  description = "Key that encrypts the shell log group."
  value       = module.encryption.key_arn
}

output "log_group_name" {
  description = "Log group the founder can read."
  value       = module.logging.log_group_name
}

output "task_role_arn" {
  description = "Role a future app task assumes. It does not administer the account."
  value       = module.identity.task_role_arn
}

output "cluster_arn" {
  description = "Empty cluster. The first sitting does not start a service or a database."
  value       = module.runtime.cluster_arn
}
