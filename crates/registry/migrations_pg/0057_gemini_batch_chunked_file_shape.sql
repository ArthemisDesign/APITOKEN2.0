-- Expand the dormant Gemini batch file row to admit chunk-backed storage.
--
-- Migration 0055's anonymous row-shape CHECK predates the chunk table from 0056 and requires every
-- active file to carry one legacy inline ciphertext. Drop only that narrowing CHECK by its catalog
-- definition, preserve every column/data row, and replace it with a named shape that distinguishes
-- legacy inline rows from the chunked writer Stage 2 will introduce. No runtime currently writes
-- either form.

ALTER TABLE gemini_batch_files
    ADD COLUMN IF NOT EXISTS storage_kind text;

DO $$
DECLARE
    constraint_name text;
BEGIN
    SELECT conname INTO constraint_name
      FROM pg_constraint
     WHERE conrelid = 'gemini_batch_files'::regclass
       AND contype = 'c'
       AND pg_get_constraintdef(oid) LIKE '%state = ''processing''%'
       AND pg_get_constraintdef(oid) LIKE '%blob_plaintext_len = size_bytes%';
    IF constraint_name IS NOT NULL THEN
        EXECUTE format('ALTER TABLE gemini_batch_files DROP CONSTRAINT %I', constraint_name);
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'gemini_batch_files_storage_shape'
           AND conrelid = 'gemini_batch_files'::regclass
    ) THEN
        ALTER TABLE gemini_batch_files
            ADD CONSTRAINT gemini_batch_files_storage_shape
            CHECK (
                storage_kind IS NULL
                OR storage_kind IN ('inline_legacy', 'chunked')
            ) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
         WHERE conname = 'gemini_batch_files_state_storage_shape'
           AND conrelid = 'gemini_batch_files'::regclass
    ) THEN
        ALTER TABLE gemini_batch_files
            ADD CONSTRAINT gemini_batch_files_state_storage_shape
            CHECK (
                (
                    state = 'processing'
                    AND failure_class IS NULL
                    AND (storage_kind IS NULL OR storage_kind = 'chunked')
                    AND blob_key_id IS NULL
                    AND blob_nonce IS NULL
                    AND blob_ciphertext IS NULL
                    AND blob_plaintext_len IS NULL
                    AND blob_digest IS NULL
                )
                OR (
                    state = 'active'
                    AND failure_class IS NULL
                    AND (
                        (
                            (storage_kind IS NULL OR storage_kind = 'inline_legacy')
                            AND blob_key_id IS NOT NULL
                            AND blob_nonce IS NOT NULL
                            AND blob_ciphertext IS NOT NULL
                            AND blob_plaintext_len = size_bytes
                            AND blob_digest IS NOT NULL
                        )
                        OR (
                            storage_kind = 'chunked'
                            AND blob_key_id IS NULL
                            AND blob_nonce IS NULL
                            AND blob_ciphertext IS NULL
                            AND blob_plaintext_len IS NULL
                            AND blob_digest IS NULL
                        )
                    )
                )
                OR (
                    state = 'failed'
                    AND failure_class IS NOT NULL
                    AND (storage_kind IS NULL OR storage_kind = 'chunked')
                    AND blob_key_id IS NULL
                    AND blob_nonce IS NULL
                    AND blob_ciphertext IS NULL
                    AND blob_plaintext_len IS NULL
                    AND blob_digest IS NULL
                )
            ) NOT VALID;
    END IF;
END $$;

-- Existing 0055 rows, if any were inserted by a schema probe, carry NULL storage_kind. Classify
-- only structurally complete rows; production has no runtime writer yet. This is metadata repair,
-- not customer payload materialization.
UPDATE gemini_batch_files
   SET storage_kind = CASE
       WHEN state = 'active' AND blob_ciphertext IS NOT NULL THEN 'inline_legacy'
       ELSE 'chunked'
   END
 WHERE storage_kind IS NULL;

ALTER TABLE gemini_batch_files
    VALIDATE CONSTRAINT gemini_batch_files_storage_shape;
ALTER TABLE gemini_batch_files
    VALIDATE CONSTRAINT gemini_batch_files_state_storage_shape;
ALTER TABLE gemini_batch_files
    ALTER COLUMN storage_kind SET NOT NULL;

CREATE INDEX IF NOT EXISTS gemini_batch_files_account_state
    ON gemini_batch_files(account_id, state, file_id)
    WHERE state IN ('processing', 'active');

INSERT INTO engine_schema_migrations(version) VALUES (57)
ON CONFLICT (version) DO NOTHING;
