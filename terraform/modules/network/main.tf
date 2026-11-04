variable "name_prefix" { type = string }
variable "environment" { type = string }
variable "cidr_block" { type = string }
variable "az_count" { type = number }
variable "enable_nat" { type = bool }
variable "enable_public_ingress" { type = bool }
variable "enable_flow_logs" { type = bool }
variable "flow_log_retention_days" { type = number }
variable "kms_key_arn" { type = string }

data "aws_availability_zones" "available" {
  state = "available"
}

locals {
  azs          = slice(data.aws_availability_zones.available.names, 0, var.az_count)
  open_edge    = var.enable_nat || var.enable_public_ingress
}

resource "aws_vpc" "shell" {
  cidr_block           = var.cidr_block
  enable_dns_support   = true
  enable_dns_hostnames = true

  tags = {
    Name = "${var.name_prefix}-${var.environment}"
  }
}

resource "aws_subnet" "public" {
  count                   = local.open_edge ? var.az_count : 0
  vpc_id                  = aws_vpc.shell.id
  availability_zone       = local.azs[count.index]
  cidr_block              = cidrsubnet(var.cidr_block, 4, count.index)
  map_public_ip_on_launch = false

  tags = {
    Name = "${var.name_prefix}-${var.environment}-edge-${count.index}"
    tier = "edge"
  }
}

resource "aws_subnet" "private" {
  count             = var.az_count
  vpc_id            = aws_vpc.shell.id
  availability_zone = local.azs[count.index]
  cidr_block        = cidrsubnet(var.cidr_block, 4, count.index + 8)

  tags = {
    Name = "${var.name_prefix}-${var.environment}-app-${count.index}"
    tier = "app"
  }
}

resource "aws_internet_gateway" "shell" {
  count  = local.open_edge ? 1 : 0
  vpc_id = aws_vpc.shell.id
}

resource "aws_route_table" "public" {
  count  = local.open_edge ? 1 : 0
  vpc_id = aws_vpc.shell.id

  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.shell[0].id
  }
}

resource "aws_route_table_association" "public" {
  count          = local.open_edge ? var.az_count : 0
  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public[0].id
}

resource "aws_eip" "nat" {
  count  = var.enable_nat ? 1 : 0
  domain = "vpc"
}

resource "aws_nat_gateway" "shell" {
  count         = var.enable_nat ? 1 : 0
  allocation_id = aws_eip.nat[0].id
  subnet_id     = aws_subnet.public[0].id
  depends_on    = [aws_internet_gateway.shell]
}

resource "aws_route_table" "private" {
  vpc_id = aws_vpc.shell.id

  dynamic "route" {
    for_each = var.enable_nat ? [1] : []
    content {
      cidr_block     = "0.0.0.0/0"
      nat_gateway_id = aws_nat_gateway.shell[0].id
    }
  }
}

resource "aws_route_table_association" "private" {
  count          = var.az_count
  subnet_id      = aws_subnet.private[count.index].id
  route_table_id = aws_route_table.private.id
}

resource "aws_security_group" "app" {
  name        = "${var.name_prefix}-${var.environment}-app"
  description = "App tasks. Inbound from the internet stays off unless enable_public_ingress is true."
  vpc_id      = aws_vpc.shell.id

  egress {
    description = "HTTPS out"
    from_port   = 443
    to_port     = 443
    protocol    = "tcp"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "aws_vpc_security_group_ingress_rule" "public" {
  count             = var.enable_public_ingress ? 1 : 0
  security_group_id = aws_security_group.app.id
  cidr_ipv4         = "0.0.0.0/0"
  ip_protocol       = "tcp"
  from_port         = 443
  to_port           = 443
  description       = "Public HTTPS. Off in the example tfvars."
}

resource "aws_iam_role" "flow" {
  count = var.enable_flow_logs ? 1 : 0
  name  = "${var.name_prefix}-${var.environment}-flow"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Principal = { Service = "vpc-flow-logs.amazonaws.com" }
      Action    = "sts:AssumeRole"
    }]
  })
}

resource "aws_iam_role_policy" "flow" {
  count = var.enable_flow_logs ? 1 : 0
  name  = "flow-logs"
  role  = aws_iam_role.flow[0].id
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect = "Allow"
      Action = [
        "logs:CreateLogStream",
        "logs:PutLogEvents",
        "logs:DescribeLogGroups",
        "logs:DescribeLogStreams",
      ]
      Resource = "*"
    }]
  })
}

resource "aws_cloudwatch_log_group" "flow" {
  count             = var.enable_flow_logs ? 1 : 0
  name              = "/${var.name_prefix}/${var.environment}/flow"
  retention_in_days = var.flow_log_retention_days
  kms_key_id        = var.kms_key_arn
}

resource "aws_flow_log" "shell" {
  count           = var.enable_flow_logs ? 1 : 0
  vpc_id          = aws_vpc.shell.id
  traffic_type    = "ALL"
  iam_role_arn    = aws_iam_role.flow[0].arn
  log_destination = aws_cloudwatch_log_group.flow[0].arn
}

output "vpc_id" {
  value = aws_vpc.shell.id
}

output "private_subnet_ids" {
  value = aws_subnet.private[*].id
}

output "app_security_group_id" {
  value = aws_security_group.app.id
}
