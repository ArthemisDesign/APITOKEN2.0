-- Lightweight Grafana rollups for request usage dimensions.
--
-- Migration 0061 preserves the full privacy-bounded drilldown views, but their broad GROUP BY over
-- the fact/usage join can exceed the monitoring PostgreSQL parallel shared-memory budget. Grafana's
-- top-level cards only need closed top-N rollups. These expand-only views aggregate one narrow axis
-- at a time, which avoids the parallel hash overflow and keeps every base-table detail outside the
-- monitoring role's direct access.

CREATE VIEW request_fact_usage_top_customer_model_daily AS
SELECT
    (to_timestamp(f.admitted_at) AT TIME ZONE 'UTC')::date AS usage_day,
    f.account_id,
    f.key_id,
    f.client_kind,
    COALESCE(f.requested_model, 'unknown') AS requested_model,
    COALESCE(f.executable_model, 'unknown') AS executable_model,
    f.provider_plane,
    COUNT(*)::bigint AS request_count,
    COUNT(u.request_id)::bigint AS billable_request_count,
    COALESCE(SUM(u.input_tokens + u.output_tokens), 0)::bigint AS tokens,
    COALESCE(SUM(u.real_nano), 0)::bigint AS real_nano,
    COALESCE(SUM(u.charge_nano), 0)::bigint AS charge_nano
FROM request_facts f
LEFT JOIN usage_events u ON u.request_id = f.billing_request_id
GROUP BY
    usage_day, f.account_id, f.key_id, f.client_kind, requested_model, executable_model,
    f.provider_plane;

CREATE VIEW request_fact_usage_top_client_daily AS
SELECT
    (to_timestamp(f.admitted_at) AT TIME ZONE 'UTC')::date AS usage_day,
    f.client_kind,
    f.client_source,
    f.provider_plane,
    COUNT(*)::bigint AS request_count,
    COUNT(u.request_id)::bigint AS billable_request_count,
    COALESCE(SUM(u.input_tokens + u.output_tokens), 0)::bigint AS tokens,
    COALESCE(SUM(u.real_nano), 0)::bigint AS real_nano,
    COALESCE(SUM(u.charge_nano), 0)::bigint AS charge_nano
FROM request_facts f
LEFT JOIN usage_events u ON u.request_id = f.billing_request_id
GROUP BY usage_day, f.client_kind, f.client_source, f.provider_plane;

CREATE VIEW request_fact_usage_top_model_daily AS
SELECT
    (to_timestamp(f.admitted_at) AT TIME ZONE 'UTC')::date AS usage_day,
    COALESCE(f.requested_model, 'unknown') AS requested_model,
    COALESCE(f.executable_model, 'unknown') AS executable_model,
    f.provider_plane,
    COUNT(*)::bigint AS request_count,
    COUNT(u.request_id)::bigint AS billable_request_count,
    COALESCE(SUM(u.input_tokens + u.output_tokens), 0)::bigint AS tokens,
    COALESCE(SUM(u.real_nano), 0)::bigint AS real_nano,
    COALESCE(SUM(u.charge_nano), 0)::bigint AS charge_nano
FROM request_facts f
LEFT JOIN usage_events u ON u.request_id = f.billing_request_id
GROUP BY usage_day, requested_model, executable_model, f.provider_plane;

CREATE VIEW request_fact_usage_top_tool_daily AS
WITH tool_classes(tool_class, bit) AS (
    VALUES
        ('custom_function', 1),
        ('custom_tool', 2),
        ('web_search', 4),
        ('computer', 8),
        ('code_execution', 16),
        ('mcp', 32),
        ('other_reviewed', 64)
)
SELECT
    (to_timestamp(f.admitted_at) AT TIME ZONE 'UTC')::date AS usage_day,
    f.account_id,
    f.key_id,
    f.client_kind,
    COALESCE(f.executable_model, 'unknown') AS executable_model,
    c.tool_class,
    COUNT(*)::bigint AS request_count,
    COUNT(u.request_id)::bigint AS billable_request_count,
    COALESCE(SUM(u.input_tokens + u.output_tokens), 0)::bigint AS tokens,
    COALESCE(SUM(u.real_nano), 0)::bigint AS real_nano,
    COALESCE(SUM(u.charge_nano), 0)::bigint AS charge_nano
FROM request_facts f
CROSS JOIN tool_classes c
LEFT JOIN usage_events u ON u.request_id = f.billing_request_id
WHERE f.tool_classes IS NOT NULL AND (f.tool_classes & c.bit) <> 0
GROUP BY usage_day, f.account_id, f.key_id, f.client_kind, executable_model, c.tool_class;

INSERT INTO engine_schema_migrations(version) VALUES (62)
ON CONFLICT (version) DO NOTHING;
