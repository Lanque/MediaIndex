# AWS scaling path

The Terraform scaffold under [`infra/terraform`](../infra/terraform) is an
explicit, reviewable scaling path rather than a claim that MediaIndex is
production-deployed.

```text
Desktop -> API -> RDS PostgreSQL
                 |
                 +-> SQS jobs -> ECS Fargate worker -> selected S3 artifacts
                 +-> CloudWatch logs/alarms
                 +-> Secrets Manager
```

## Cost and safety defaults

- RDS, ECS tasks, and budget notifications are disabled by default.
- Worker desired count is zero until an immutable image, private subnets,
  security groups, and an approved budget exist.
- S3 blocks public access, encrypts objects, and expires temporary artifacts in
  seven days by default.
- SQS has a three-delivery redrive limit and a dead-letter queue.
- RDS is private, encrypted, and bounded to 20–50 GiB when explicitly enabled.
- The optional AWS Budget emits actual-spend alerts at 50%, 80%, and 100% to
  explicitly configured recipients.

Terraform `plan` is required for review; `apply` requires explicit cost,
network, IAM, secret, and rollback approval. Original local footage is never
part of the infrastructure state or default upload path.
