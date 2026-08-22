# AWS infrastructure scaffold

The Terraform module under `terraform/` describes the intended cloud scaling
path without making a deployment or creating billable services from the
repository. It includes private selected-artifact storage, an SQS job queue and
DLQ, CloudWatch logs/alarms, Secrets Manager integration, least-privilege
worker IAM, optional ECS Fargate, optional RDS PostgreSQL, and optional budget
notifications.

## Validate locally

```bash
cd infra/terraform
terraform fmt -check
terraform init
terraform validate
terraform plan -var-file=terraform.tfvars
```

Copy `terraform.tfvars.example` to a local ignored `terraform.tfvars`. Review
network IDs, image digest, IAM permissions, monthly budget, and rollback steps
before enabling RDS, ECS, or budget notifications. `worker_desired_count` is
zero by default and the expensive services are disabled by default.

Original footage is not a Terraform-managed artifact. S3 is limited to
selected temporary outputs with encryption, public access blocked, and a short
lifecycle.
