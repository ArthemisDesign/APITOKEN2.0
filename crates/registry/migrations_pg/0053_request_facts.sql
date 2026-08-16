-- Request observability: dormant request-fact storage (docs/engine/REQUEST_OBSERVABILITY.md).
-- Expand-only: introduces the table and its query-proven indexes without any runtime dependency.
-- No foreign keys by contract section 11: facts are analytics rows with an explicit 30-day
-- retention; they must not cascade away with shorter-lived lifecycle rows or keep them alive.
-- Ids are engine UUIDv4 strings stored as text, matching reservations.request_id and
-- usage_events.request_id. Timestamps are epoch seconds (bigint), matching created_ts/ts.
-- Closed-vocabulary CHECKs exist only where the producing code maps every value totally;
-- anything unparsed or unsupported is stored as 'unknown'/NULL, never fabricated (section 3.7).
CREATE TABLE IF NOT EXISTS request_facts (
    fact_id bigserial PRIMARY KEY,
    logical_request_id text NOT NULL,
    billing_request_id text UNIQUE,
    execution_group_id text,
    attempt integer NOT NULL DEFAULT 1 CHECK (attempt > 0),
    account_id text NOT NULL,
    key_id text NOT NULL,
    client_kind text NOT NULL DEFAULT 'unknown' CHECK (client_kind IN
        ('claude_code','opencode','codex_cli','cursor','sdk','custom','unknown')),
    client_source text NOT NULL DEFAULT 'unknown' CHECK (client_source IN
        ('explicit','heuristic','unknown')),
    client_version text,
    provider_plane text NOT NULL,
    route_class text NOT NULL,
    request_class text NOT NULL,
    requested_model text,
    executable_model text,
    stream_flag boolean NOT NULL DEFAULT false,
    tools_declared_count integer CHECK (tools_declared_count IS NULL OR tools_declared_count >= 0),
    tool_classes integer NOT NULL DEFAULT 0,
    tool_choice_mode text CHECK (tool_choice_mode IS NULL OR tool_choice_mode IN
        ('auto','required','none','named','unknown')),
    parallel_tools_requested boolean,
    tool_results_in_input boolean NOT NULL DEFAULT false,
    tool_calls_in_output boolean NOT NULL DEFAULT false,
    structured_output_flag boolean NOT NULL DEFAULT false,
    reasoning_flag boolean NOT NULL DEFAULT false,
    service_tier text,
    input_modalities integer NOT NULL DEFAULT 0,
    output_modalities integer NOT NULL DEFAULT 0,
    admitted_at bigint NOT NULL,
    delivery_started_at bigint,
    first_public_byte_at bigint,
    terminal_at bigint,
    http_status_code integer CHECK (http_status_code IS NULL OR http_status_code BETWEEN 100 AND 599),
    provider_terminal_class text CHECK (provider_terminal_class IS NULL OR provider_terminal_class IN
        ('success','client_error','quota','auth','timeout','transport','upstream_error','protocol_error','unknown')),
    billing_outcome text CHECK (billing_outcome IS NULL OR billing_outcome IN
        ('winner','loser','zero_metered','canceled','reconciled','not_applicable','unknown')),
    downstream_disconnect boolean,
    upstream_request_id text,
    internal_attempt_count integer CHECK (internal_attempt_count IS NULL OR internal_attempt_count >= 0),
    failure_class text,
    schema_version integer NOT NULL DEFAULT 1
);
-- Section 11 index set: query-proven only. Mutable terminal columns stay out of indexes.
CREATE INDEX IF NOT EXISTS request_facts_logical_attempt_idx
    ON request_facts (logical_request_id, attempt);
CREATE INDEX IF NOT EXISTS request_facts_account_admitted_idx
    ON request_facts (account_id, admitted_at DESC, fact_id);
CREATE INDEX IF NOT EXISTS request_facts_admitted_idx
    ON request_facts (admitted_at);

INSERT INTO engine_schema_migrations(version) VALUES (53)
ON CONFLICT (version) DO NOTHING;
