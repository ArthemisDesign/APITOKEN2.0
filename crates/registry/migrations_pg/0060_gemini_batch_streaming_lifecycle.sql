-- Expand-only authority for bounded 100k-item admission, resumable 2 GiB files, atomic output
-- publication, and lifecycle maintenance. Schema-59 and older runtimes ignore every new object and
-- continue using the existing live job/item/blob/file authorities unchanged.

ALTER TABLE gemini_batch_files
    ADD COLUMN IF NOT EXISTS received_bytes bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS next_chunk_index bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS chunk_count bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS chunk_manifest_digest bytea,
    ADD COLUMN IF NOT EXISTS completed_ts bigint,
    ADD COLUMN IF NOT EXISTS payload_deleted_ts bigint;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname='gemini_batch_files_streaming_progress_shape'
           AND conrelid='gemini_batch_files'::regclass
    ) THEN
        ALTER TABLE gemini_batch_files
            ADD CONSTRAINT gemini_batch_files_streaming_progress_shape CHECK (
                received_bytes BETWEEN 0 AND size_bytes
                AND next_chunk_index >= 0
                AND chunk_count = next_chunk_index
                AND (chunk_manifest_digest IS NULL OR octet_length(chunk_manifest_digest)=32)
                AND (completed_ts IS NULL OR completed_ts >= create_ts)
                AND (payload_deleted_ts IS NULL OR payload_deleted_ts >= create_ts)
            ) NOT VALID;
    END IF;
END $$;

WITH progress AS (
    SELECT f.file_id,
           COALESCE(SUM(c.plaintext_len),0)::bigint AS received_bytes,
           COUNT(c.*)::bigint AS chunk_count
      FROM gemini_batch_files f
 LEFT JOIN gemini_batch_file_chunks c USING(file_id)
     GROUP BY f.file_id
)
UPDATE gemini_batch_files f
   SET received_bytes=p.received_bytes,
       next_chunk_index=p.chunk_count,
       chunk_count=p.chunk_count,
       completed_ts=CASE WHEN f.state='active' THEN f.update_ts ELSE f.completed_ts END
  FROM progress p
 WHERE p.file_id=f.file_id;

ALTER TABLE gemini_batch_files
    VALIDATE CONSTRAINT gemini_batch_files_streaming_progress_shape;
CREATE INDEX IF NOT EXISTS gemini_batch_files_processing_expiry
    ON gemini_batch_files(expiration_ts,file_id) WHERE state='processing';

ALTER TABLE gemini_batch_jobs
    ADD COLUMN IF NOT EXISTS terminal_items_ts bigint,
    ADD COLUMN IF NOT EXISTS output_state text,
    ADD COLUMN IF NOT EXISTS payload_deleted_ts bigint,
    ADD COLUMN IF NOT EXISTS tombstone_expiration_ts bigint;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname='gemini_batch_jobs_streaming_lifecycle_shape'
           AND conrelid='gemini_batch_jobs'::regclass
    ) THEN
        ALTER TABLE gemini_batch_jobs
            ADD CONSTRAINT gemini_batch_jobs_streaming_lifecycle_shape CHECK (
                (output_state IS NULL OR output_state IN ('pending','building','ready','failed'))
                AND (terminal_items_ts IS NULL OR terminal_items_ts >= create_ts)
                AND (payload_deleted_ts IS NULL OR payload_deleted_ts >= create_ts)
                AND (tombstone_expiration_ts IS NULL OR tombstone_expiration_ts >= create_ts)
            ) NOT VALID;
    END IF;
END $$;

UPDATE gemini_batch_jobs
   SET terminal_items_ts=COALESCE(terminal_items_ts,completed_ts),
       output_state=CASE
           WHEN input_kind='file' AND output_file_id IS NOT NULL THEN 'ready'
           WHEN input_kind='file' AND completed_ts IS NOT NULL THEN 'failed'
           ELSE output_state
       END
 WHERE completed_ts IS NOT NULL;
ALTER TABLE gemini_batch_jobs
    VALIDATE CONSTRAINT gemini_batch_jobs_streaming_lifecycle_shape;
CREATE INDEX IF NOT EXISTS gemini_batch_jobs_output_pending
    ON gemini_batch_jobs(update_ts,job_id)
    WHERE input_kind='file' AND terminal_items_ts IS NOT NULL
      AND completed_ts IS NULL AND output_state IN ('pending','failed');

-- Staging rows are ciphertext-only and invisible to list/get/dispatch. No money is reserved until
-- the final publish transaction promotes the complete staged set into the existing live tables.
CREATE TABLE IF NOT EXISTS gemini_batch_admissions (
    admission_id text PRIMARY KEY CHECK (admission_id <> ''),
    job_id text NOT NULL UNIQUE CHECK (job_id <> ''),
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    creator_key_id text NOT NULL CHECK (creator_key_id <> ''),
    public_model text NOT NULL CHECK (public_model <> '' AND octet_length(public_model) <= 255),
    display_name text NOT NULL CHECK (display_name <> '' AND octet_length(display_name) <= 512),
    idempotency_digest bytea CHECK (idempotency_digest IS NULL OR octet_length(idempotency_digest)=32),
    canonical_request_digest bytea CHECK (canonical_request_digest IS NULL OR octet_length(canonical_request_digest)=32),
    priority bigint NOT NULL DEFAULT 0,
    input_kind text NOT NULL CHECK (input_kind IN ('inline','file')),
    input_file_id text REFERENCES gemini_batch_files(file_id) ON DELETE RESTRICT,
    schema_version integer NOT NULL CHECK (schema_version > 0),
    encryption_policy_version integer NOT NULL CHECK (encryption_policy_version > 0),
    state text NOT NULL CHECK (state IN ('staging','sealed','committed','aborted')),
    next_item_index bigint NOT NULL DEFAULT 0 CHECK (next_item_index BETWEEN 0 AND 100000),
    aggregate_hold_nano bigint NOT NULL DEFAULT 0 CHECK (aggregate_hold_nano >= 0),
    aggregate_output_tokens bigint NOT NULL DEFAULT 0 CHECK (aggregate_output_tokens >= 0),
    create_ts bigint NOT NULL CHECK (create_ts > 0),
    deadline_ts bigint NOT NULL CHECK (deadline_ts > create_ts),
    expires_ts bigint NOT NULL CHECK (expires_ts > create_ts),
    update_ts bigint NOT NULL CHECK (update_ts >= create_ts),
    CHECK ((input_kind='file')=(input_file_id IS NOT NULL))
);
CREATE UNIQUE INDEX IF NOT EXISTS gemini_batch_admissions_account_idempotency
    ON gemini_batch_admissions(account_id,idempotency_digest)
    WHERE idempotency_digest IS NOT NULL AND state IN ('staging','sealed','committed');
CREATE INDEX IF NOT EXISTS gemini_batch_admissions_expiry
    ON gemini_batch_admissions(expires_ts,admission_id)
    WHERE state IN ('staging','sealed','aborted');

CREATE TABLE IF NOT EXISTS gemini_batch_admission_items (
    admission_id text NOT NULL REFERENCES gemini_batch_admissions(admission_id) ON DELETE CASCADE,
    item_index bigint NOT NULL CHECK (item_index BETWEEN 0 AND 99999),
    request_id text NOT NULL UNIQUE CHECK (request_id <> ''),
    logical_request_id text NOT NULL UNIQUE CHECK (logical_request_id <> ''),
    execution_group_id text NOT NULL UNIQUE CHECK (execution_group_id <> ''),
    client_key text CHECK (client_key IS NULL OR octet_length(client_key) <= 512),
    request_digest bytea NOT NULL CHECK (octet_length(request_digest)=32),
    input_file_id text REFERENCES gemini_batch_files(file_id) ON DELETE RESTRICT,
    hold_nano bigint NOT NULL CHECK (hold_nano >= 0),
    requested_output_tokens bigint NOT NULL CHECK (requested_output_tokens > 0),
    payable_multiplier_bp bigint NOT NULL CHECK (payable_multiplier_bp BETWEEN 0 AND 10000),
    priced_ts bigint NOT NULL CHECK (priced_ts > 0),
    tariff_family text NOT NULL CHECK (tariff_family <> '' AND octet_length(tariff_family) <= 255),
    tariff_version bigint NOT NULL CHECK (tariff_version > 0),
    tariff_schedule_id text NOT NULL CHECK (tariff_schedule_id <> '' AND octet_length(tariff_schedule_id) <= 255),
    request_key_id text NOT NULL CHECK (request_key_id <> '' AND octet_length(request_key_id) <= 128),
    request_nonce bytea NOT NULL CHECK (octet_length(request_nonce)=24),
    request_ciphertext bytea NOT NULL,
    request_plaintext_len bigint NOT NULL CHECK (request_plaintext_len >= 0),
    request_plaintext_digest bytea NOT NULL CHECK (octet_length(request_plaintext_digest)=32),
    metadata_key_id text CHECK (metadata_key_id IS NULL OR (metadata_key_id <> '' AND octet_length(metadata_key_id) <= 128)),
    metadata_nonce bytea CHECK (metadata_nonce IS NULL OR octet_length(metadata_nonce)=24),
    metadata_ciphertext bytea,
    metadata_plaintext_len bigint CHECK (metadata_plaintext_len IS NULL OR metadata_plaintext_len >= 0),
    metadata_plaintext_digest bytea CHECK (metadata_plaintext_digest IS NULL OR octet_length(metadata_plaintext_digest)=32),
    retention_ts bigint NOT NULL CHECK (retention_ts > 0),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    PRIMARY KEY (admission_id,item_index),
    CHECK (octet_length(request_ciphertext)::bigint=request_plaintext_len+16),
    CHECK (num_nonnulls(metadata_key_id,metadata_nonce,metadata_ciphertext,metadata_plaintext_len,metadata_plaintext_digest) IN (0,5)),
    CHECK (metadata_ciphertext IS NULL OR octet_length(metadata_ciphertext)::bigint=metadata_plaintext_len+16)
);

CREATE TABLE IF NOT EXISTS gemini_batch_admission_item_files (
    admission_id text NOT NULL,
    item_index bigint NOT NULL,
    ordinal integer NOT NULL CHECK (ordinal >= 0),
    file_id text NOT NULL REFERENCES gemini_batch_files(file_id) ON DELETE RESTRICT,
    PRIMARY KEY (admission_id,item_index,ordinal),
    UNIQUE (admission_id,item_index,file_id),
    FOREIGN KEY (admission_id,item_index)
        REFERENCES gemini_batch_admission_items(admission_id,item_index) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS gemini_batch_admission_item_files_file
    ON gemini_batch_admission_item_files(file_id,admission_id,item_index);

CREATE TABLE IF NOT EXISTS gemini_batch_output_builds (
    job_id text PRIMARY KEY REFERENCES gemini_batch_jobs(job_id) ON DELETE RESTRICT,
    file_id text NOT NULL UNIQUE REFERENCES gemini_batch_files(file_id) ON DELETE RESTRICT,
    generation bigint NOT NULL CHECK (generation > 0),
    state text NOT NULL CHECK (state IN ('pending','building','ready','failed')),
    owner_instance text,
    owner_epoch bigint,
    lease_until bigint,
    next_item_index bigint NOT NULL DEFAULT 0 CHECK (next_item_index BETWEEN 0 AND 100000),
    next_chunk_index bigint NOT NULL DEFAULT 0 CHECK (next_chunk_index >= 0),
    plaintext_bytes bigint NOT NULL DEFAULT 0 CHECK (plaintext_bytes >= 0),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts >= created_ts),
    last_error_class text CHECK (last_error_class IS NULL OR octet_length(last_error_class) <= 128),
    CHECK ((owner_instance IS NULL AND owner_epoch IS NULL AND lease_until IS NULL)
        OR (owner_instance IS NOT NULL AND owner_epoch IS NOT NULL AND lease_until IS NOT NULL))
);
CREATE INDEX IF NOT EXISTS gemini_batch_output_builds_claim
    ON gemini_batch_output_builds(state,lease_until,updated_ts,job_id)
    WHERE state IN ('pending','building','failed');

INSERT INTO engine_schema_migrations(version) VALUES (60)
ON CONFLICT (version) DO NOTHING;
