variable "project_name" {
  type        = string
  description = "Short project identifier used in resource prefixes."
  default     = "mediaindex"
}

variable "environment" {
  type        = string
  description = "Deployment environment."
  default     = "dev"
}

variable "aws_region" {
  type        = string
  description = "AWS region for the optional deployment."
  default     = "eu-north-1"
}

variable "artifact_retention_days" {
  type        = number
  description = "Days before selected temporary artifacts expire."
  default     = 7

  validation {
    condition     = var.artifact_retention_days >= 1 && var.artifact_retention_days <= 30
    error_message = "Artifact retention must be between 1 and 30 days."
  }
}

variable "worker_image" {
  type        = string
  description = "Immutable worker container image URI. Empty keeps ECS disabled."
  default     = ""
}

variable "private_subnet_ids" {
  type        = list(string)
  description = "Private subnet IDs for optional ECS worker tasks."
  default     = []
}

variable "worker_security_group_id" {
  type        = string
  description = "Security group for optional ECS worker tasks."
  default     = ""
}

variable "worker_desired_count" {
  type        = number
  description = "Initial worker count; zero avoids accidental workload spend."
  default     = 0

  validation {
    condition     = var.worker_desired_count >= 0 && var.worker_desired_count <= 10
    error_message = "Worker desired count must be between 0 and 10."
  }
}

variable "enable_rds" {
  type        = bool
  description = "Create the optional PostgreSQL instance. Keep false until cost approval."
  default     = false
}

variable "database_subnet_ids" {
  type        = list(string)
  description = "At least two private subnet IDs when RDS is enabled."
  default     = []

  validation {
    condition     = !var.enable_rds || length(var.database_subnet_ids) >= 2
    error_message = "RDS requires at least two database subnet IDs."
  }
}

variable "database_security_group_id" {
  type        = string
  description = "Security group allowing API-to-RDS traffic when RDS is enabled."
  default     = ""
}

variable "database_instance_class" {
  type        = string
  description = "Smallest approved RDS class for the environment."
  default     = "db.t4g.micro"
}

variable "budget_limit_usd" {
  type        = number
  description = "Monthly AWS budget ceiling."
  default     = 25

  validation {
    condition     = var.budget_limit_usd > 0 && var.budget_limit_usd <= 1000
    error_message = "Budget must be greater than zero and no more than 1000 USD."
  }
}

variable "enable_budget" {
  type        = bool
  description = "Create AWS Budget notifications. Enable only with approved email recipients."
  default     = false
}

variable "budget_alert_emails" {
  type        = set(string)
  description = "Recipients for 50%, 80%, and 100% actual-spend alerts."
  default     = []
}
