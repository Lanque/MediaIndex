# ADR 0004: Deterministic search precedes semantic search

- Status: accepted
- Date: 2026-08-21

## Context

Users need dependable technical filters and file metadata search. Semantic search is valuable, but it introduces model, embedding, indexing, and cost complexity.

## Decision

Build local keyword/metadata search and ordinary filters first. Add PostgreSQL full-text search after sync, then add pgvector semantic search as a separate capability.

## Consequences

- core search remains useful offline;
- deterministic filters remain explainable and testable;
- semantic similarity can be evaluated without changing filter semantics;
- the product avoids making an AI assistant the primary interface.
