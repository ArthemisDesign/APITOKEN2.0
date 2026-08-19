-- Dormant, expand-only persistence foundation for Gemini Batch Mode.
--
-- The currently serving runtime neither reads nor writes these tables. The dependent authority and
-- worker are delivered only after this migration is production-GREEN. Customer request/result/file
-- bytes are opaque ciphertext (`bytea`); PostgreSQL never stores their plaintext. Job statistics are
-- deliberately absent from `gemini_batch_jobs`: every public batchStats projection must be derived
-- from item rows.

CREATE TABLE IF NOT EXISTS gemini_batch_files (
    file_id text PRIMARY KEY CHECK (file_id <> ''),
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    display_name text NOT NULL DEFAULT '' CHECK (octet_length(display_name) <= 512),
    mime_type text NOT NULL CHECK (mime_type <> '' AND octet_length(mime_type) <= 255),
    size_bytes bigint NOT NULL CHECK (size_bytes >= 0),
    sha256_digest bytea NOT NULL CHECK (octet_length(sha256_digest) = 32),
    source_kind text NOT NULL CHECK (source_kind IN ('client_upload', 'batch_output')),
    state text NOT NULL CHECK (state IN ('processing', 'active', 'failed')),
    failure_class text CHECK (failure_class IS NULL OR octet_length(failure_class) <= 128),
    blob_key_id text CHECK (
        blob_key_id IS NULL OR (blob_key_id <> '' AND octet_length(blob_key_id) <= 128)
    ),
    blob_nonce bytea CHECK (blob_nonce IS NULL OR octet_length(blob_nonce) = 24),
    blob_ciphertext bytea CHECK (
        blob_ciphertext IS NULL OR octet_length(blob_ciphertext) >= 16
    ),
    blob_plaintext_len bigint CHECK (blob_plaintext_len IS NULL OR blob_plaintext_len >= 0),
    blob_digest bytea CHECK (blob_digest IS NULL OR octet_length(blob_digest) = 32),
    create_ts bigint NOT NULL CHECK (create_ts > 0),
    update_ts bigint NOT NULL CHECK (update_ts >= create_ts),
    expiration_ts bigint NOT NULL CHECK (expiration_ts >= create_ts),
    CHECK (
        (state = 'processing' AND failure_class IS NULL AND blob_key_id IS NULL
            AND blob_nonce IS NULL AND blob_ciphertext IS NULL
            AND blob_plaintext_len IS NULL AND blob_digest IS NULL)
        OR (state = 'active' AND failure_class IS NULL AND blob_key_id IS NOT NULL
            AND blob_nonce IS NOT NULL AND blob_ciphertext IS NOT NULL
            AND blob_plaintext_len = size_bytes AND blob_digest IS NOT NULL)
        OR (state = 'failed' AND failure_class IS NOT NULL AND blob_key_id IS NULL
            AND blob_nonce IS NULL AND blob_ciphertext IS NULL
            AND blob_plaintext_len IS NULL AND blob_digest IS NULL)
    )
);

CREATE INDEX IF NOT EXISTS gemini_batch_files_account_created
    ON gemini_batch_files(account_id, create_ts DESC, file_id);
CREATE INDEX IF NOT EXISTS gemini_batch_files_expiration
    ON gemini_batch_files(expiration_ts, file_id);

CREATE TABLE IF NOT EXISTS gemini_batch_jobs (
    job_id text PRIMARY KEY CHECK (job_id <> ''),
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    creator_key_id text NOT NULL CHECK (creator_key_id <> ''),
    public_model text NOT NULL CHECK (public_model <> '' AND octet_length(public_model) <= 255),
    display_name text NOT NULL CHECK (display_name <> '' AND octet_length(display_name) <= 512),
    canonical_request_digest bytea NOT NULL CHECK (octet_length(canonical_request_digest) = 32),
    idempotency_digest bytea CHECK (
        idempotency_digest IS NULL OR octet_length(idempotency_digest) = 32
    ),
    priority bigint NOT NULL DEFAULT 0,
    input_kind text NOT NULL CHECK (input_kind IN ('inline', 'file')),
    input_file_id text REFERENCES gemini_batch_files(file_id) ON DELETE RESTRICT,
    output_file_id text REFERENCES gemini_batch_files(file_id) ON DELETE RESTRICT,
    schema_version integer NOT NULL CHECK (schema_version > 0),
    encryption_policy_version integer NOT NULL CHECK (encryption_policy_version > 0),
    cancel_requested_ts bigint,
    job_failure_class text CHECK (
        job_failure_class IS NULL OR octet_length(job_failure_class) <= 128
    ),
    create_ts bigint NOT NULL CHECK (create_ts > 0),
    update_ts bigint NOT NULL CHECK (update_ts >= create_ts),
    deadline_ts bigint NOT NULL CHECK (deadline_ts > create_ts),
    completed_ts bigint,
    delete_ts bigint,
    result_expiration_ts bigint NOT NULL CHECK (result_expiration_ts > create_ts),
    CHECK ((input_kind = 'file') = (input_file_id IS NOT NULL)),
    CHECK (input_kind = 'file' OR output_file_id IS NULL),
    CHECK (cancel_requested_ts IS NULL OR cancel_requested_ts >= create_ts),
    CHECK (completed_ts IS NULL OR completed_ts >= create_ts),
    CHECK (delete_ts IS NULL OR delete_ts >= create_ts)
);

CREATE UNIQUE INDEX IF NOT EXISTS gemini_batch_jobs_account_idempotency
    ON gemini_batch_jobs(account_id, idempotency_digest)
    WHERE idempotency_digest IS NOT NULL;
CREATE INDEX IF NOT EXISTS gemini_batch_jobs_account_created
    ON gemini_batch_jobs(account_id, create_ts DESC, job_id);
CREATE INDEX IF NOT EXISTS gemini_batch_jobs_deadline
    ON gemini_batch_jobs(deadline_ts, job_id)
    WHERE completed_ts IS NULL;
CREATE INDEX IF NOT EXISTS gemini_batch_jobs_result_expiration
    ON gemini_batch_jobs(result_expiration_ts, job_id)
    WHERE completed_ts IS NOT NULL;

CREATE TABLE IF NOT EXISTS gemini_batch_items (
    job_id text NOT NULL REFERENCES gemini_batch_jobs(job_id) ON DELETE RESTRICT,
    item_index bigint NOT NULL CHECK (item_index >= 0),
    request_id text NOT NULL UNIQUE CHECK (request_id <> ''),
    logical_request_id text NOT NULL UNIQUE CHECK (logical_request_id <> ''),
    execution_group_id text NOT NULL UNIQUE CHECK (execution_group_id <> ''),
    client_key text CHECK (client_key IS NULL OR octet_length(client_key) <= 512),
    request_digest bytea NOT NULL CHECK (octet_length(request_digest) = 32),
    input_file_id text REFERENCES gemini_batch_files(file_id) ON DELETE RESTRICT,
    hold_nano bigint NOT NULL CHECK (hold_nano >= 0),
    provider text NOT NULL DEFAULT 'google' CHECK (provider = 'google'),
    payable_multiplier_bp bigint NOT NULL CHECK (payable_multiplier_bp BETWEEN 0 AND 10000),
    priced_ts bigint NOT NULL CHECK (priced_ts > 0),
    tariff_family text NOT NULL CHECK (
        tariff_family <> '' AND octet_length(tariff_family) <= 255
    ),
    tariff_version bigint NOT NULL CHECK (tariff_version > 0),
    tariff_schedule_id text NOT NULL CHECK (
        tariff_schedule_id <> '' AND octet_length(tariff_schedule_id) <= 255
    ),
    state text NOT NULL CHECK (state IN (
        'queued', 'claimed', 'dispatching', 'settlement_pending',
        'succeeded', 'failed', 'indeterminate', 'canceled'
    )),
    terminal_class text CHECK (terminal_class IS NULL OR terminal_class IN (
        'success', 'client_error', 'quota', 'auth', 'timeout', 'transport',
        'upstream_error', 'protocol_error', 'indeterminate', 'canceled', 'expired'
    )),
    next_attempt_ts bigint NOT NULL DEFAULT 0,
    worker_instance text,
    worker_epoch bigint,
    claim_generation bigint NOT NULL DEFAULT 0 CHECK (claim_generation >= 0),
    lease_until bigint,
    selected_profile_id text,
    dispatch_intent_ts bigint,
    actual_send_ts bigint,
    actual_send_evidence text CHECK (
        actual_send_evidence IS NULL OR actual_send_evidence IN ('not_sent', 'sent', 'ambiguous')
    ),
    attempt_count integer NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    usage_input_tokens bigint CHECK (usage_input_tokens IS NULL OR usage_input_tokens >= 0),
    usage_tool_prompt_tokens bigint CHECK (
        usage_tool_prompt_tokens IS NULL OR usage_tool_prompt_tokens >= 0
    ),
    usage_audio_input_tokens bigint CHECK (
        usage_audio_input_tokens IS NULL OR usage_audio_input_tokens >= 0
    ),
    usage_cached_input_tokens bigint CHECK (
        usage_cached_input_tokens IS NULL OR usage_cached_input_tokens >= 0
    ),
    usage_cached_audio_input_tokens bigint CHECK (
        usage_cached_audio_input_tokens IS NULL OR usage_cached_audio_input_tokens >= 0
    ),
    usage_output_tokens bigint CHECK (usage_output_tokens IS NULL OR usage_output_tokens >= 0),
    usage_thinking_output_tokens bigint CHECK (
        usage_thinking_output_tokens IS NULL OR usage_thinking_output_tokens >= 0
    ),
    usage_image_output_tokens bigint CHECK (
        usage_image_output_tokens IS NULL OR usage_image_output_tokens >= 0
    ),
    usage_search_queries bigint CHECK (
        usage_search_queries IS NULL OR usage_search_queries >= 0
    ),
    usage_grounded_search_prompts bigint CHECK (
        usage_grounded_search_prompts IS NULL OR usage_grounded_search_prompts >= 0
    ),
    settlement_id text UNIQUE,
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts >= created_ts),
    terminal_ts bigint,
    PRIMARY KEY (job_id, item_index),
    CHECK (
        (worker_instance IS NULL AND worker_epoch IS NULL AND lease_until IS NULL)
        OR (worker_instance IS NOT NULL AND worker_epoch IS NOT NULL AND lease_until IS NOT NULL)
    ),
    CHECK (actual_send_ts IS NULL OR dispatch_intent_ts IS NOT NULL),
    CHECK (
        (actual_send_ts IS NULL AND actual_send_evidence IS DISTINCT FROM 'sent')
        OR (actual_send_ts IS NOT NULL AND actual_send_evidence = 'sent')
    ),
    CHECK (actual_send_evidence IS NULL OR dispatch_intent_ts IS NOT NULL),
    CHECK (
        num_nonnulls(
            usage_input_tokens, usage_tool_prompt_tokens, usage_audio_input_tokens,
            usage_cached_input_tokens, usage_cached_audio_input_tokens, usage_output_tokens,
            usage_thinking_output_tokens, usage_image_output_tokens,
            usage_search_queries, usage_grounded_search_prompts
        ) IN (0, 10)
    ),
    CHECK (
        usage_tool_prompt_tokens IS NULL OR usage_tool_prompt_tokens <= usage_input_tokens
    ),
    CHECK (
        usage_cached_audio_input_tokens IS NULL
        OR usage_cached_audio_input_tokens <= usage_cached_input_tokens
    ),
    CHECK (
        usage_thinking_output_tokens IS NULL
        OR usage_thinking_output_tokens <= usage_output_tokens
    ),
    CHECK (terminal_ts IS NULL OR terminal_ts >= created_ts),
    CHECK (
        (state IN ('succeeded', 'failed', 'indeterminate', 'canceled'))
        = (terminal_class IS NOT NULL AND terminal_ts IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS gemini_batch_items_job_state
    ON gemini_batch_items(job_id, state, item_index);
CREATE INDEX IF NOT EXISTS gemini_batch_items_dispatch
    ON gemini_batch_items(state, next_attempt_ts, job_id, item_index)
    WHERE state IN ('queued', 'claimed');
CREATE INDEX IF NOT EXISTS gemini_batch_items_owner_lease
    ON gemini_batch_items(worker_instance, worker_epoch, lease_until)
    WHERE state IN ('claimed', 'dispatching', 'settlement_pending');

CREATE TABLE IF NOT EXISTS gemini_batch_item_files (
    job_id text NOT NULL,
    item_index bigint NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    file_id text NOT NULL REFERENCES gemini_batch_files(file_id) ON DELETE RESTRICT,
    PRIMARY KEY (job_id, item_index, ordinal),
    UNIQUE (job_id, item_index, file_id),
    FOREIGN KEY (job_id, item_index)
        REFERENCES gemini_batch_items(job_id, item_index) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS gemini_batch_item_files_file
    ON gemini_batch_item_files(file_id, job_id, item_index);

CREATE TABLE IF NOT EXISTS gemini_batch_blobs (
    job_id text NOT NULL,
    item_index bigint NOT NULL,
    kind text NOT NULL CHECK (kind IN ('request', 'metadata', 'result', 'error')),
    key_id text NOT NULL CHECK (key_id <> '' AND octet_length(key_id) <= 128),
    nonce bytea NOT NULL CHECK (octet_length(nonce) = 24),
    ciphertext bytea NOT NULL,
    plaintext_len bigint NOT NULL CHECK (plaintext_len >= 0),
    plaintext_digest bytea NOT NULL CHECK (octet_length(plaintext_digest) = 32),
    retention_ts bigint NOT NULL CHECK (retention_ts > 0),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    PRIMARY KEY (job_id, item_index, kind),
    FOREIGN KEY (job_id, item_index)
        REFERENCES gemini_batch_items(job_id, item_index) ON DELETE RESTRICT,
    CHECK (retention_ts >= created_ts)
);
CREATE INDEX IF NOT EXISTS gemini_batch_blobs_retention
    ON gemini_batch_blobs(retention_ts, job_id, item_index, kind);

CREATE TABLE IF NOT EXISTS gemini_batch_settlement_outbox (
    request_id text PRIMARY KEY,
    job_id text NOT NULL,
    item_index bigint NOT NULL,
    disposition text NOT NULL CHECK (disposition IN (
        'settle', 'cancel', 'indeterminate', 'expire'
    )),
    actual_nano bigint NOT NULL CHECK (actual_nano >= 0),
    charge_basis_nano bigint NOT NULL CHECK (charge_basis_nano >= 0),
    real_nano bigint NOT NULL CHECK (real_nano >= 0),
    usage_input_tokens bigint CHECK (usage_input_tokens IS NULL OR usage_input_tokens >= 0),
    usage_tool_prompt_tokens bigint CHECK (
        usage_tool_prompt_tokens IS NULL OR usage_tool_prompt_tokens >= 0
    ),
    usage_audio_input_tokens bigint CHECK (
        usage_audio_input_tokens IS NULL OR usage_audio_input_tokens >= 0
    ),
    usage_cached_input_tokens bigint CHECK (
        usage_cached_input_tokens IS NULL OR usage_cached_input_tokens >= 0
    ),
    usage_cached_audio_input_tokens bigint CHECK (
        usage_cached_audio_input_tokens IS NULL OR usage_cached_audio_input_tokens >= 0
    ),
    usage_output_tokens bigint CHECK (usage_output_tokens IS NULL OR usage_output_tokens >= 0),
    usage_thinking_output_tokens bigint CHECK (
        usage_thinking_output_tokens IS NULL OR usage_thinking_output_tokens >= 0
    ),
    usage_image_output_tokens bigint CHECK (
        usage_image_output_tokens IS NULL OR usage_image_output_tokens >= 0
    ),
    usage_search_queries bigint CHECK (
        usage_search_queries IS NULL OR usage_search_queries >= 0
    ),
    usage_grounded_search_prompts bigint CHECK (
        usage_grounded_search_prompts IS NULL OR usage_grounded_search_prompts >= 0
    ),
    result_kind text NOT NULL CHECK (result_kind IN ('response', 'error')),
    terminal_state text NOT NULL CHECK (terminal_state IN (
        'succeeded', 'failed', 'indeterminate', 'canceled'
    )),
    state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'done', 'failed')),
    attempts bigint NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_ts bigint NOT NULL DEFAULT 0,
    last_error_class text CHECK (
        last_error_class IS NULL OR octet_length(last_error_class) <= 128
    ),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts >= created_ts),
    committed_ts bigint,
    UNIQUE (job_id, item_index),
    FOREIGN KEY (job_id, item_index)
        REFERENCES gemini_batch_items(job_id, item_index) ON DELETE RESTRICT,
    CHECK (
        num_nonnulls(
            usage_input_tokens, usage_tool_prompt_tokens, usage_audio_input_tokens,
            usage_cached_input_tokens, usage_cached_audio_input_tokens, usage_output_tokens,
            usage_thinking_output_tokens, usage_image_output_tokens,
            usage_search_queries, usage_grounded_search_prompts
        ) IN (0, 10)
    ),
    CHECK (
        usage_tool_prompt_tokens IS NULL OR usage_tool_prompt_tokens <= usage_input_tokens
    ),
    CHECK (
        usage_cached_audio_input_tokens IS NULL
        OR usage_cached_audio_input_tokens <= usage_cached_input_tokens
    ),
    CHECK (
        usage_thinking_output_tokens IS NULL
        OR usage_thinking_output_tokens <= usage_output_tokens
    ),
    CHECK (committed_ts IS NULL OR committed_ts >= created_ts)
);
CREATE INDEX IF NOT EXISTS gemini_batch_settlement_outbox_pending
    ON gemini_batch_settlement_outbox(state, next_attempt_ts, created_ts, request_id)
    WHERE state IN ('pending', 'failed');

CREATE TABLE IF NOT EXISTS gemini_batch_profile_leases (
    profile_id text PRIMARY KEY CHECK (profile_id <> ''),
    job_id text NOT NULL,
    item_index bigint NOT NULL,
    worker_instance text NOT NULL CHECK (worker_instance <> ''),
    worker_epoch bigint NOT NULL,
    claim_generation bigint NOT NULL CHECK (claim_generation > 0),
    lease_until bigint NOT NULL,
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts >= created_ts),
    FOREIGN KEY (job_id, item_index)
        REFERENCES gemini_batch_items(job_id, item_index) ON DELETE RESTRICT
);
CREATE UNIQUE INDEX IF NOT EXISTS gemini_batch_profile_leases_item
    ON gemini_batch_profile_leases(job_id, item_index);
CREATE INDEX IF NOT EXISTS gemini_batch_profile_leases_expiry
    ON gemini_batch_profile_leases(lease_until, profile_id);

INSERT INTO engine_schema_migrations(version) VALUES (55)
ON CONFLICT (version) DO NOTHING;
