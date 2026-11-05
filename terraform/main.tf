module "encryption" {
  source      = "./modules/encryption"
  name_prefix = var.name_prefix
  environment = var.environment
}

module "logging" {
  source          = "./modules/logging"
  name_prefix     = var.name_prefix
  environment     = var.environment
  kms_key_arn     = module.encryption.key_arn
  retention_days  = var.flow_log_retention_days
}

module "network" {
  source                  = "./modules/network"
  name_prefix             = var.name_prefix
  environment             = var.environment
  cidr_block              = var.cidr_block
  az_count                = var.az_count
  enable_nat              = var.enable_nat
  enable_public_ingress   = var.enable_public_ingress
  enable_flow_logs        = var.enable_flow_logs
  flow_log_retention_days = var.flow_log_retention_days
  kms_key_arn             = module.encryption.key_arn
}

module "identity" {
  source        = "./modules/identity"
  name_prefix   = var.name_prefix
  environment   = var.environment
  log_group_arn = module.logging.log_group_arn
}

module "runtime" {
  source             = "./modules/runtime"
  name_prefix        = var.name_prefix
  environment        = var.environment
  private_subnet_ids = module.network.private_subnet_ids
  task_role_arn      = module.identity.task_role_arn
}
