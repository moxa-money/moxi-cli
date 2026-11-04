variable "name_prefix" { type = string }
variable "environment" { type = string }
variable "log_group_arn" { type = string }

data "aws_iam_policy_document" "task" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["ecs-tasks.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "task" {
  name               = "${var.name_prefix}-${var.environment}-task"
  assume_role_policy = data.aws_iam_policy_document.task.json
}

resource "aws_iam_role" "execution" {
  name               = "${var.name_prefix}-${var.environment}-execution"
  assume_role_policy = data.aws_iam_policy_document.task.json
}

data "aws_iam_policy_document" "execution" {
  statement {
    actions = [
      "logs:CreateLogStream",
      "logs:PutLogEvents",
    ]
    resources = ["${var.log_group_arn}:*"]
  }
}

resource "aws_iam_role_policy" "execution" {
  name   = "shell-logs"
  role   = aws_iam_role.execution.id
  policy = data.aws_iam_policy_document.execution.json
}

output "task_role_arn" {
  value = aws_iam_role.task.arn
}

output "execution_role_arn" {
  value = aws_iam_role.execution.arn
}
