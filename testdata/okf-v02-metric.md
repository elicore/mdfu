---
type: Metric
title: Monthly Active Users
description: Tracks monthly active users of the platform.
tags: [growth, kpi, monthly]
status: verified
created: 2024-05-01
generated:
  by: data-pipeline
  at: "2024-06-01T10:00:00Z"
verified:
  by: alice
  at: "2024-06-02T12:30:00Z"
sources:
  - id: warehouse
    resource: snowflake://analytics/mau
    title: MAU Warehouse Table
    author: data-team
  - id: dashboard
    resource: https://dash.example.com/mau
    title: MAU Dashboard
    author: alice
---

# Monthly Active Users

MAU grew 12% month over month, driven by onboarding improvements.
