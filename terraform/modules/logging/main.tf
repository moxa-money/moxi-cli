variable "name_prefix" { type = string }
variable "environment" { type = string }
variable "kms_key_arn" { type = string }
variable "retention_days" { type = number }

resource "aws_cloudwatch_log_group" "shell" {
  name              = "/${var.name_prefix}/${var.environment}/shell"
  retention_in_days = var.retention_days
  kms_key_id        = var.kms_key_arn
}

output "log_group_name" {
  value = aws_cloudwatch_log_group.shell.name
}

output "log_group_arn" {
  value = aws_cloudwatch_log_group.shell.arn
}
