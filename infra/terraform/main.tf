data "aws_caller_identity" "current" {}

locals {
  prefix              = "${var.project_name}-${var.environment}"
  enable_worker       = var.worker_image != "" && length(var.private_subnet_ids) > 0 && var.worker_security_group_id != ""
  enable_budget_alert = var.enable_budget && length(var.budget_alert_emails) > 0
}

resource "aws_s3_bucket" "selected_artifacts" {
  bucket_prefix = "${local.prefix}-artifacts-"
}

resource "aws_s3_bucket_public_access_block" "selected_artifacts" {
  bucket = aws_s3_bucket.selected_artifacts.id

  block_public_acls       = true
  block_public_policy     = true
  ignore_public_acls      = true
  restrict_public_buckets = true
}

resource "aws_s3_bucket_server_side_encryption_configuration" "selected_artifacts" {
  bucket = aws_s3_bucket.selected_artifacts.id

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_s3_bucket_lifecycle_configuration" "selected_artifacts" {
  bucket = aws_s3_bucket.selected_artifacts.id

  rule {
    id     = "expire-selected-temporary-artifacts"
    status = "Enabled"

    expiration {
      days = var.artifact_retention_days
    }

    abort_incomplete_multipart_upload {
      days_after_initiation = 1
    }
  }
}

resource "aws_sqs_queue" "dead_letter" {
  name                      = "${local.prefix}-dead-letter"
  message_retention_seconds = 1_209_600
}

resource "aws_sqs_queue" "jobs" {
  name                       = "${local.prefix}-jobs"
  visibility_timeout_seconds = 900
  message_retention_seconds  = 345_600
  receive_wait_time_seconds  = 20
  redrive_policy = jsonencode({
    deadLetterTargetArn = aws_sqs_queue.dead_letter.arn
    maxReceiveCount     = 3
  })
}

resource "aws_cloudwatch_log_group" "worker" {
  name              = "/mediaindex/${var.environment}/worker"
  retention_in_days = 14
}

resource "aws_secretsmanager_secret" "database" {
  count = var.enable_rds ? 1 : 0
  name  = "${local.prefix}/database"
}

data "aws_iam_policy_document" "worker_assume_role" {
  statement {
    effect = "Allow"

    principals {
      type        = "Service"
      identifiers = ["ecs-tasks.amazonaws.com"]
    }

    actions = ["sts:AssumeRole"]
  }
}

resource "aws_iam_role" "worker" {
  name               = "${local.prefix}-worker"
  assume_role_policy = data.aws_iam_policy_document.worker_assume_role.json
}

resource "aws_iam_role" "worker_execution" {
  name               = "${local.prefix}-worker-execution"
  assume_role_policy = data.aws_iam_policy_document.worker_assume_role.json
}

resource "aws_iam_role_policy_attachment" "worker_execution" {
  role       = aws_iam_role.worker_execution.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy"
}

data "aws_iam_policy_document" "worker" {
  statement {
    sid       = "JobQueueAccess"
    effect    = "Allow"
    actions   = ["sqs:ReceiveMessage", "sqs:DeleteMessage", "sqs:ChangeMessageVisibility", "sqs:GetQueueAttributes"]
    resources = [aws_sqs_queue.jobs.arn]
  }

  statement {
    sid       = "DeadLetterWrite"
    effect    = "Allow"
    actions   = ["sqs:SendMessage"]
    resources = [aws_sqs_queue.dead_letter.arn]
  }

  statement {
    sid       = "SelectedArtifactAccess"
    effect    = "Allow"
    actions   = ["s3:GetObject", "s3:PutObject", "s3:AbortMultipartUpload"]
    resources = ["${aws_s3_bucket.selected_artifacts.arn}/*"]
  }

  statement {
    sid       = "WorkerLogs"
    effect    = "Allow"
    actions   = ["logs:CreateLogStream", "logs:PutLogEvents"]
    resources = ["${aws_cloudwatch_log_group.worker.arn}:*"]
  }

  dynamic "statement" {
    for_each = var.enable_rds ? [1] : []

    content {
      sid       = "DatabaseSecretRead"
      effect    = "Allow"
      actions   = ["secretsmanager:GetSecretValue"]
      resources = [aws_secretsmanager_secret.database[0].arn]
    }
  }
}

resource "aws_iam_role_policy" "worker" {
  name   = "${local.prefix}-worker-policy"
  role   = aws_iam_role.worker.id
  policy = data.aws_iam_policy_document.worker.json
}

resource "aws_ecs_cluster" "worker" {
  name = "${local.prefix}-worker"
}

resource "aws_ecs_task_definition" "worker" {
  count = local.enable_worker ? 1 : 0

  family                   = "${local.prefix}-worker"
  cpu                      = "256"
  memory                   = "512"
  network_mode             = "awsvpc"
  requires_compatibilities = ["FARGATE"]
  execution_role_arn       = aws_iam_role.worker_execution.arn
  task_role_arn            = aws_iam_role.worker.arn

  container_definitions = jsonencode([
    {
      name      = "worker"
      image     = var.worker_image
      essential = true
      command   = ["python", "-m", "worker"]
      logConfiguration = {
        logDriver = "awslogs"
        options = {
          awslogs-group         = aws_cloudwatch_log_group.worker.name
          awslogs-region        = var.aws_region
          awslogs-stream-prefix = "worker"
        }
      }
    }
  ])
}

resource "aws_ecs_service" "worker" {
  count = local.enable_worker ? 1 : 0

  name            = "${local.prefix}-worker"
  cluster         = aws_ecs_cluster.worker.id
  task_definition = aws_ecs_task_definition.worker[0].arn
  desired_count   = var.worker_desired_count
  launch_type     = "FARGATE"

  network_configuration {
    subnets         = var.private_subnet_ids
    security_groups = [var.worker_security_group_id]
  }
}

resource "aws_cloudwatch_metric_alarm" "queue_age" {
  alarm_name          = "${local.prefix}-queue-visible-messages"
  comparison_operator = "GreaterThanThreshold"
  evaluation_periods  = 2
  metric_name         = "ApproximateNumberOfMessagesVisible"
  namespace           = "AWS/SQS"
  period              = 300
  statistic           = "Average"
  threshold           = 100
  alarm_description   = "Review worker capacity or stuck messages before scaling."
  treat_missing_data  = "notBreaching"

  dimensions = {
    QueueName = aws_sqs_queue.jobs.name
  }
}

resource "aws_budgets_budget" "monthly" {
  count = local.enable_budget_alert ? 1 : 0

  name         = "${local.prefix}-monthly"
  budget_type  = "COST"
  limit_amount = tostring(var.budget_limit_usd)
  limit_unit   = "USD"
  time_unit    = "MONTHLY"

  dynamic "notification" {
    for_each = setproduct(var.budget_alert_emails, [50, 80, 100])

    content {
      comparison_operator        = "GREATER_THAN"
      threshold                  = notification.value[1]
      threshold_type             = "PERCENTAGE"
      notification_type          = "ACTUAL"
      subscriber_email_addresses = [notification.value[0]]
    }
  }
}

resource "aws_db_subnet_group" "database" {
  count      = var.enable_rds ? 1 : 0
  name       = "${local.prefix}-database"
  subnet_ids = var.database_subnet_ids
}

resource "aws_db_instance" "database" {
  count = var.enable_rds ? 1 : 0

  identifier             = "${local.prefix}-database"
  engine                 = "postgres"
  engine_version         = "16"
  instance_class         = var.database_instance_class
  allocated_storage      = 20
  max_allocated_storage  = 50
  db_name                = "mediaindex"
  username               = "mediaindex"
  manage_master_user_password = true
  db_subnet_group_name   = aws_db_subnet_group.database[0].name
  vpc_security_group_ids = var.database_security_group_id == "" ? [] : [var.database_security_group_id]
  storage_encrypted      = true
  publicly_accessible    = false
  backup_retention_period = 7
  deletion_protection    = false
  skip_final_snapshot    = true

  lifecycle {
    precondition {
      condition     = var.database_security_group_id != ""
      error_message = "RDS requires an explicit private database security group."
    }
  }
}
