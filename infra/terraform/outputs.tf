output "artifact_bucket_name" {
  value       = aws_s3_bucket.selected_artifacts.bucket
  description = "Private bucket for selected temporary artifacts only."
}

output "jobs_queue_url" {
  value       = aws_sqs_queue.jobs.url
  description = "Queue URL for explicit processing jobs."
}

output "dead_letter_queue_url" {
  value       = aws_sqs_queue.dead_letter.url
  description = "Dead-letter queue URL for diagnostics and recovery."
}

output "database_endpoint" {
  value       = var.enable_rds ? aws_db_instance.database[0].address : null
  description = "Optional private RDS endpoint."
}
