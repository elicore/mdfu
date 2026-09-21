---
type: Metric
title: Hermes Ingest Latency
description: Tracks p99 ingest latency of the Hermes pipeline.
tags: [hermes, ingest]
status: verified
created: 2026-09-01
meta: '{"created_by": "Hermes", "created_at": "2026-09-01T10:00:00Z"}'
generated:
  by: hermes-pipeline
  at: "2026-09-01T10:05:00Z"
verified:
  by: hermes
  at: "2026-09-02T09:00:00Z"
sources:
  - id: ingest-dashboard
    resource: https://metrics.example.com/hermes/ingest
    title: Hermes Ingest Dashboard
    author: hermes
---

# Hermes Ingest Latency

p99 ingest latency held under 250ms through the Hermes rollout.
