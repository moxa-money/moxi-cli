variable "name_prefix" {
  description = "Short name prepended to every resource. Letters, numbers, and hyphens."
  type        = string
}

variable "environment" {
  description = "Stage label stored on tags. It is not an account id."
  type        = string
}

variable "region" {
  description = "AWS region for the shell."
  type        = string
}

variable "cidr_block" {
  description = "IPv4 CIDR for the VPC."
  type        = string
}

variable "az_count" {
  description = "Number of availability zones. Two is the closed-shell default."
  type        = number
}

variable "enable_nat" {
  description = "Place one NAT gateway so private subnets can reach the internet on the way out."
  type        = bool
}

variable "enable_public_ingress" {
  description = "When false, the app security group has no inbound rule from the internet."
  type        = bool
}

variable "enable_flow_logs" {
  description = "Send VPC flow logs to the encrypted log group."
  type        = bool
}

variable "flow_log_retention_days" {
  description = "Days to keep flow logs and the shell log group."
  type        = number
}

variable "tags" {
  description = "Tags applied to every resource. Do not put an account id or a secret here."
  type        = map(string)
}
