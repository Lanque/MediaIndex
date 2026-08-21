# ADR 0003: Cloud processing is explicit and asynchronous

- Status: accepted
- Date: 2026-08-21

## Context

Transcription, visual analysis, embedding generation, and preview creation can be expensive. Running them automatically for every file would create unpredictable cost and poor failure visibility.

## Decision

The user explicitly selects clips and operations. The backend records a job and queues it for asynchronous worker processing.

## Consequences

- cloud cost is tied to an intentional action;
- queue delivery, retries, duplicate messages, and dead-letter behavior are first-class;
- job history can explain what happened;
- the initial user experience is search and workflow, not an AI chatbot;
- selected cloud jobs may upload media bytes only when the job requires them.
