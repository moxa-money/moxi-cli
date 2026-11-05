variable "name_prefix" { type = string }
variable "environment" { type = string }

# The cluster stays empty. These inputs record where a later service would run
# and which role it would assume. They do not start a task.
variable "private_subnet_ids" { type = list(string) }
variable "task_role_arn" { type = string }

resource "aws_ecs_cluster" "shell" {
  name = "${var.name_prefix}-${var.environment}"
}

output "cluster_arn" {
  value = aws_ecs_cluster.shell.arn
}

output "private_subnet_ids" {
  value = var.private_subnet_ids
}

output "task_role_arn" {
  value = var.task_role_arn
}
