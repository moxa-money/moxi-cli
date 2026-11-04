variable "name_prefix" { type = string }
variable "environment" { type = string }

resource "aws_ecs_cluster" "shell" {
  name = "${var.name_prefix}-${var.environment}"
}

output "cluster_arn" {
  value = aws_ecs_cluster.shell.arn
}
