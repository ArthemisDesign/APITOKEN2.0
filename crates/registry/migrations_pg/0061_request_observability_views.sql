-- Read-only aggregate reporting views for the private Grafana request-usage datasource.
-- Expand-only: opaque engine account and non-secret key identity are exposed, but no email, key
-- label, raw API key, request identity, prompt, tool name, schema, argument, or result is exposed.
-- The views preserve exact model text only as a database report dimension; they are never exported
-- as Prometheus labels.

CREATE VIEW request_fact_usage_daily AS
WITH facts AS (
    SELECT
        (to_timestamp(f.admitted_at) AT TIME ZONE 'UTC')::date AS usage_day,
        f.account_id,
        f.key_id,
        f.client_kind,
        f.client_source,
        COALESCE(f.requested_model, 'unknown') AS requested_model,
        COALESCE(f.executable_model, 'unknown') AS executable_model,
        f.provider_plane,
        f.route_class,
        f.request_class,
        f.stream_flag,
        COALESCE(f.tools_declared_count, -1) AS tools_declared_count,
        f.tool_classes,
        COALESCE(f.tool_choice_mode, 'unknown') AS tool_choice_mode,
        f.parallel_tools_requested,
        f.tool_results_in_input,
        f.tool_calls_in_output,
        f.structured_output_flag AS structured_output,
        f.reasoning_flag AS reasoning,
        CASE
            WHEN f.tools_declared_count IS NULL THEN 'unknown'
            WHEN f.tools_declared_count = 0 THEN 'zero'
            WHEN f.tools_declared_count BETWEEN 1 AND 3 THEN '1-3'
            WHEN f.tools_declared_count BETWEEN 4 AND 10 THEN '4-10'
            ELSE '11+'
        END AS tools_declared_bucket,
        u.input_tokens,
        u.output_tokens,
        u.cache_read_tokens,
        u.cache_write_5m_tokens,
        u.cache_write_1h_tokens,
        u.web_search_requests,
        u.real_nano,
        u.charge_nano,
        (u.request_id IS NOT NULL)::integer AS billable_request
    FROM request_facts f
    LEFT JOIN usage_events u ON u.request_id = f.billing_request_id
)
SELECT
    usage_day,
    account_id,
    key_id,
    client_kind,
    client_source,
    requested_model,
    executable_model,
    provider_plane,
    route_class,
    request_class,
    stream_flag,
    tools_declared_bucket,
    tool_classes,
    tool_choice_mode,
    parallel_tools_requested,
    tool_results_in_input,
    tool_calls_in_output,
    structured_output,
    reasoning,
    COUNT(*)::bigint AS request_count,
    SUM(billable_request)::bigint AS billable_request_count,
    COALESCE(SUM(input_tokens), 0)::bigint AS input_tokens,
    COALESCE(SUM(output_tokens), 0)::bigint AS output_tokens,
    COALESCE(SUM(cache_read_tokens), 0)::bigint AS cache_read_tokens,
    COALESCE(SUM(cache_write_5m_tokens), 0)::bigint AS cache_write_5m_tokens,
    COALESCE(SUM(cache_write_1h_tokens), 0)::bigint AS cache_write_1h_tokens,
    COALESCE(SUM(web_search_requests), 0)::bigint AS web_search_requests,
    COALESCE(SUM(real_nano), 0)::bigint AS real_nano,
    COALESCE(SUM(charge_nano), 0)::bigint AS charge_nano
FROM facts
GROUP BY
    usage_day, account_id, key_id, client_kind, client_source, requested_model, executable_model,
    provider_plane, route_class, request_class, stream_flag, tools_declared_bucket,
    tool_classes, tool_choice_mode, parallel_tools_requested, tool_results_in_input,
    tool_calls_in_output, structured_output, reasoning;

CREATE VIEW request_fact_tool_usage_daily AS
WITH tool_classes(tool_class, bit) AS (
    VALUES
        ('custom_function', 1),
        ('custom_tool', 2),
        ('web_search', 4),
        ('computer', 8),
        ('code_execution', 16),
        ('mcp', 32),
        ('other_reviewed', 64)
), facts AS (
    SELECT
        (to_timestamp(f.admitted_at) AT TIME ZONE 'UTC')::date AS usage_day,
        f.account_id,
        f.key_id,
        f.client_kind,
        f.client_source,
        COALESCE(f.requested_model, 'unknown') AS requested_model,
        COALESCE(f.executable_model, 'unknown') AS executable_model,
        f.provider_plane,
        f.route_class,
        f.request_class,
        c.tool_class,
        u.input_tokens,
        u.output_tokens,
        u.real_nano,
        u.charge_nano,
        (u.request_id IS NOT NULL)::integer AS billable_request
    FROM request_facts f
    CROSS JOIN tool_classes c
    LEFT JOIN usage_events u ON u.request_id = f.billing_request_id
    WHERE f.tool_classes IS NOT NULL AND (f.tool_classes & c.bit) <> 0
)
SELECT
    usage_day,
    account_id,
    key_id,
    client_kind,
    client_source,
    requested_model,
    executable_model,
    provider_plane,
    route_class,
    request_class,
    tool_class,
    COUNT(*)::bigint AS request_count,
    SUM(billable_request)::bigint AS billable_request_count,
    COALESCE(SUM(input_tokens), 0)::bigint AS input_tokens,
    COALESCE(SUM(output_tokens), 0)::bigint AS output_tokens,
    COALESCE(SUM(real_nano), 0)::bigint AS real_nano,
    COALESCE(SUM(charge_nano), 0)::bigint AS charge_nano
FROM facts
GROUP BY
    usage_day, account_id, key_id, client_kind, client_source, requested_model, executable_model,
    provider_plane, route_class, request_class, tool_class;

INSERT INTO engine_schema_migrations(version) VALUES (61)
ON CONFLICT (version) DO NOTHING;
