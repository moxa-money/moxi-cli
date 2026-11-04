variable "name_prefix" { type = string }
variable "environment" { type = string }

data "aws_caller_identity" "current" {}
data "aws_region" "current" {}

resource "aws_kms_key" "shell" {
  description             = "${var.name_prefix}-${var.environment} shell logs"
  deletion_window_in_days = 30
  enable_key_rotation     = true
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [
      {
        Sid       = "AccountAdmin"
        Effect    = "Allow"
        Principal = { AWS = "arn:aws:iam::${data.aws_caller_identity.current.account_id}:root" }
        Action    = "kms:*"
        Resource  = "*"
      },
      {
        Sid       = "ShellLogs"
        Effect    = "Allow"
        Principal = { Service = "logs.${data.aws_region.current.name}.amazonaws.com" }
        Action = [
          "kms:Encrypt",
          "kms:Decrypt",
          "kms:ReEncrypt*",
          "kms:GenerateDataKey*",
          "kms:DescribeKey",
        ]
        Resource = "*"
        Condition = {
          ArnLike = {
            "kms:EncryptionContext:aws:logs:arn" = "arn:aws:logs:${data.aws_region.current.name}:${data.aws_caller_identity.current.account_id}:*"
          }
        }
      }
    ]
  })
}

resource "aws_kms_alias" "shell" {
  name          = "alias/${var.name_prefix}-${var.environment}-shell"
  target_key_id = aws_kms_key.shell.key_id
}

output "key_arn" {
  value = aws_kms_key.shell.arn
}
