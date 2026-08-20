//! Dedicated encryption domain for durable Gemini Batch customer data.
//!
//! The codec has no configuration or I/O side effects. Callers provide the already-parsed
//! keyring and the complete durable identity used as AEAD associated data.

use anyhow::{anyhow, bail, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    Key, XChaCha20Poly1305, XNonce,
};
use registry::{GeminiBatchEncryptedBlob, GeminiBatchFileChunk, GeminiBatchFileCompletion};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fmt};
use zeroize::{Zeroize, Zeroizing};

const AAD_DOMAIN: &[u8] = b"apitoken:gemini-batch-data:v1\0";
const FILE_CHUNK_MANIFEST_DOMAIN: &[u8] = b"apitoken:gemini-batch-file-chunks:v1\0";
const NONCE_LEN: usize = 24;
const TAG_LEN: usize = 16;
const MAX_KEY_ID_LEN: usize = 128;

/// Parseable, rotation-aware keyring dedicated to Gemini Batch customer data.
///
/// The format is `active_kid;kid:base64url-key[,kid:base64url-key...]`. Keys are exactly 32 bytes,
/// encoded with unpadded URL-safe base64. Encryption always uses `active_kid`; decryption selects
/// the envelope's `key_id`, so retained old keys remain readable during rotation.
#[derive(Clone)]
pub struct GeminiBatchDataKeyring {
    active_key_id: String,
    keys: HashMap<String, [u8; 32]>,
}

impl Drop for GeminiBatchDataKeyring {
    fn drop(&mut self) {
        for key in self.keys.values_mut() {
            key.zeroize();
        }
    }
}

impl fmt::Debug for GeminiBatchDataKeyring {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeminiBatchDataKeyring")
            .field("key_count", &self.keys.len())
            .field("active_key_id", &"REDACTED")
            .field("keys", &"REDACTED")
            .finish()
    }
}

impl GeminiBatchDataKeyring {
    pub fn parse(specification: &str) -> Result<Self> {
        let (active_key_id, entries) = specification
            .split_once(';')
            .ok_or_else(|| anyhow!("Gemini Batch data keyring must be active_kid;kid:base64url"))?;
        validate_component(active_key_id, "active key id")?;

        let mut keys = HashMap::new();
        for entry in entries
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            let (key_id, encoded) = entry
                .split_once(':')
                .ok_or_else(|| anyhow!("Gemini Batch data key entry must be kid:base64url"))?;
            validate_component(key_id, "key id")?;
            let mut decoded = Zeroizing::new(
                URL_SAFE_NO_PAD
                    .decode(encoded)
                    .context("decode Gemini Batch data key")?,
            );
            if decoded.len() != 32 {
                bail!("Gemini Batch data key must be exactly 32 bytes")
            }
            let mut key = [0u8; 32];
            key.copy_from_slice(&decoded);
            decoded.zeroize();
            if keys.insert(key_id.to_owned(), key).is_some() {
                bail!("duplicate Gemini Batch data key id")
            }
        }
        if keys.is_empty() {
            bail!("Gemini Batch data keyring is empty")
        }
        if !keys.contains_key(active_key_id) {
            bail!("active Gemini Batch data key id is unavailable")
        }
        Ok(Self {
            active_key_id: active_key_id.to_owned(),
            keys,
        })
    }

    pub fn active_key_id(&self) -> &str {
        &self.active_key_id
    }

    pub fn contains(&self, key_id: &str) -> bool {
        self.keys.contains_key(key_id)
    }

    /// Encrypt a request, metadata, result, or error blob in the registry's ciphertext shape.
    pub fn encrypt_blob(
        &self,
        identity: &GeminiBatchBlobIdentity<'_>,
        plaintext: &[u8],
        retention_ts: i64,
    ) -> Result<GeminiBatchEncryptedBlob> {
        identity.validate()?;
        if retention_ts <= 0 {
            bail!("invalid Gemini Batch blob retention timestamp")
        }
        let plaintext_len =
            i64::try_from(plaintext.len()).context("Gemini Batch blob too large")?;
        let plaintext_digest = Sha256::digest(plaintext).into();
        let (key_id, nonce, ciphertext) = self.encrypt(identity.aad()?, plaintext)?;
        Ok(GeminiBatchEncryptedBlob {
            kind: identity.kind.to_owned(),
            key_id,
            nonce: nonce.to_vec(),
            ciphertext,
            plaintext_len,
            plaintext_digest,
            retention_ts,
        })
    }

    /// Authenticate and decrypt a registry blob, including its stored length and digest.
    pub fn decrypt_blob(
        &self,
        identity: &GeminiBatchBlobIdentity<'_>,
        blob: &GeminiBatchEncryptedBlob,
    ) -> Result<Zeroizing<Vec<u8>>> {
        identity.validate()?;
        if blob.kind != identity.kind
            || blob.nonce.len() != NONCE_LEN
            || blob.plaintext_len < 0
            || usize::try_from(blob.plaintext_len)
                .ok()
                .and_then(|len| len.checked_add(TAG_LEN))
                != Some(blob.ciphertext.len())
        {
            bail!("invalid Gemini Batch encrypted blob")
        }
        let plaintext =
            self.decrypt(&blob.key_id, &blob.nonce, identity.aad()?, &blob.ciphertext)?;
        verify_plaintext(&plaintext, blob.plaintext_len, &blob.plaintext_digest)?;
        Ok(plaintext)
    }

    /// Start ordered streaming encryption for a file. Each pushed slice becomes one durable chunk.
    pub fn file_encryptor<'a>(
        &'a self,
        account_id: &'a str,
        file_id: &'a str,
        schema_version: i32,
    ) -> Result<GeminiBatchFileEncryptor<'a>> {
        GeminiBatchFileEncryptor::new(self, account_id, file_id, schema_version)
    }

    /// Encrypt one resumable-upload chunk at its durable explicit index.
    pub fn encrypt_file_chunk(
        &self,
        identity: &GeminiBatchFileChunkIdentity<'_>,
        plaintext: &[u8],
        created_ts: i64,
    ) -> Result<GeminiBatchFileChunk> {
        identity.validate()?;
        if plaintext.is_empty()
            || plaintext.len() > registry::MAX_BATCH_FILE_CHUNK_BYTES as usize
            || created_ts <= 0
        {
            bail!("invalid Gemini Batch resumable file chunk")
        }
        let plaintext_len =
            i64::try_from(plaintext.len()).context("Gemini Batch chunk too large")?;
        let plaintext_digest = Sha256::digest(plaintext).into();
        let (key_id, nonce, ciphertext) = self.encrypt(identity.aad()?, plaintext)?;
        let chunk = GeminiBatchFileChunk {
            chunk_index: identity.chunk_index,
            key_id,
            nonce: nonce.to_vec(),
            ciphertext,
            plaintext_len,
            plaintext_digest,
            created_ts,
        };
        chunk.validate()?;
        Ok(chunk)
    }

    pub fn decrypt_file_chunk(
        &self,
        identity: &GeminiBatchFileChunkIdentity<'_>,
        chunk: &GeminiBatchFileChunk,
    ) -> Result<Zeroizing<Vec<u8>>> {
        identity.validate()?;
        if chunk.chunk_index != identity.chunk_index
            || chunk.nonce.len() != NONCE_LEN
            || chunk.plaintext_len < 0
            || usize::try_from(chunk.plaintext_len)
                .ok()
                .and_then(|len| len.checked_add(TAG_LEN))
                != Some(chunk.ciphertext.len())
        {
            bail!("invalid Gemini Batch encrypted file chunk")
        }
        let plaintext = self.decrypt(
            &chunk.key_id,
            &chunk.nonce,
            identity.aad()?,
            &chunk.ciphertext,
        )?;
        verify_plaintext(&plaintext, chunk.plaintext_len, &chunk.plaintext_digest)?;
        Ok(plaintext)
    }

    fn encrypt(&self, aad: Vec<u8>, plaintext: &[u8]) -> Result<(String, [u8; 24], Vec<u8>)> {
        let key = self
            .keys
            .get(&self.active_key_id)
            .ok_or_else(|| anyhow!("active Gemini Batch data key id is unavailable"))?;
        let mut nonce = [0u8; NONCE_LEN];
        getrandom::fill(&mut nonce).map_err(|_| anyhow!("operating-system CSPRNG unavailable"))?;
        let ciphertext = XChaCha20Poly1305::new(Key::from_slice(key))
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("encrypt Gemini Batch data failed"))?;
        Ok((self.active_key_id.clone(), nonce, ciphertext))
    }

    fn decrypt(
        &self,
        key_id: &str,
        nonce: &[u8],
        aad: Vec<u8>,
        ciphertext: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>> {
        let key = self
            .keys
            .get(key_id)
            .ok_or_else(|| anyhow!("Gemini Batch data key id is unavailable"))?;
        if nonce.len() != NONCE_LEN {
            bail!("invalid Gemini Batch data nonce")
        }
        let plaintext = XChaCha20Poly1305::new(Key::from_slice(key))
            .decrypt(
                XNonce::from_slice(nonce),
                Payload {
                    msg: ciphertext,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("Gemini Batch data authentication failed"))?;
        Ok(Zeroizing::new(plaintext))
    }
}

/// Complete durable identity for a non-file batch blob.
#[derive(Clone, Copy)]
pub struct GeminiBatchBlobIdentity<'a> {
    pub account_id: &'a str,
    pub job_id: &'a str,
    pub item_index: i64,
    pub kind: &'a str,
    pub schema_version: i32,
}

impl fmt::Debug for GeminiBatchBlobIdentity<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeminiBatchBlobIdentity")
            .field("identity", &"REDACTED")
            .field("schema_version", &self.schema_version)
            .finish()
    }
}

impl GeminiBatchBlobIdentity<'_> {
    fn validate(&self) -> Result<()> {
        validate_component(self.account_id, "account id")?;
        validate_component(self.job_id, "job id")?;
        validate_kind(self.kind)?;
        if self.item_index < 0 || self.schema_version <= 0 {
            bail!("invalid Gemini Batch blob identity")
        }
        Ok(())
    }

    fn aad(&self) -> Result<Vec<u8>> {
        let mut aad = Vec::new();
        aad.extend_from_slice(AAD_DOMAIN);
        encode_field(&mut aad, b"blob")?;
        encode_field(&mut aad, self.account_id.as_bytes())?;
        encode_field(&mut aad, self.job_id.as_bytes())?;
        aad.extend_from_slice(&self.item_index.to_be_bytes());
        encode_field(&mut aad, self.kind.as_bytes())?;
        aad.extend_from_slice(&self.schema_version.to_be_bytes());
        Ok(aad)
    }
}

/// Complete durable identity for one encrypted file chunk.
#[derive(Clone, Copy)]
pub struct GeminiBatchFileChunkIdentity<'a> {
    pub account_id: &'a str,
    pub file_id: &'a str,
    pub chunk_index: i64,
    pub schema_version: i32,
}

impl fmt::Debug for GeminiBatchFileChunkIdentity<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeminiBatchFileChunkIdentity")
            .field("identity", &"REDACTED")
            .field("schema_version", &self.schema_version)
            .finish()
    }
}

impl GeminiBatchFileChunkIdentity<'_> {
    fn validate(&self) -> Result<()> {
        validate_component(self.account_id, "account id")?;
        validate_component(self.file_id, "file id")?;
        if self.chunk_index < 0 || self.schema_version <= 0 {
            bail!("invalid Gemini Batch file chunk identity")
        }
        Ok(())
    }

    fn aad(&self) -> Result<Vec<u8>> {
        let mut aad = Vec::new();
        aad.extend_from_slice(AAD_DOMAIN);
        encode_field(&mut aad, b"file")?;
        encode_field(&mut aad, self.account_id.as_bytes())?;
        encode_field(&mut aad, self.file_id.as_bytes())?;
        aad.extend_from_slice(&self.chunk_index.to_be_bytes());
        encode_field(&mut aad, b"file")?;
        aad.extend_from_slice(&self.schema_version.to_be_bytes());
        Ok(aad)
    }
}

/// Incremental file encryptor producing registry chunks plus compatible completion digests.
pub struct GeminiBatchFileEncryptor<'a> {
    keyring: &'a GeminiBatchDataKeyring,
    account_id: &'a str,
    file_id: &'a str,
    schema_version: i32,
    next_chunk_index: i64,
    whole_file: Sha256,
    manifest_entries: Vec<(i64, i64, [u8; 32])>,
}

impl fmt::Debug for GeminiBatchFileEncryptor<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("GeminiBatchFileEncryptor")
            .field("identity", &"REDACTED")
            .field("next_chunk_index", &self.next_chunk_index)
            .field("digests", &"REDACTED")
            .finish()
    }
}

impl<'a> GeminiBatchFileEncryptor<'a> {
    fn new(
        keyring: &'a GeminiBatchDataKeyring,
        account_id: &'a str,
        file_id: &'a str,
        schema_version: i32,
    ) -> Result<Self> {
        GeminiBatchFileChunkIdentity {
            account_id,
            file_id,
            chunk_index: 0,
            schema_version,
        }
        .validate()?;
        Ok(Self {
            keyring,
            account_id,
            file_id,
            schema_version,
            next_chunk_index: 0,
            whole_file: Sha256::new(),
            manifest_entries: Vec::new(),
        })
    }

    /// Encrypt the next chunk. The caller persists returned chunks in this exact order.
    pub fn push_chunk(
        &mut self,
        plaintext: &[u8],
        created_ts: i64,
    ) -> Result<GeminiBatchFileChunk> {
        if plaintext.is_empty() || created_ts <= 0 {
            bail!("invalid Gemini Batch file chunk input")
        }
        let identity = GeminiBatchFileChunkIdentity {
            account_id: self.account_id,
            file_id: self.file_id,
            chunk_index: self.next_chunk_index,
            schema_version: self.schema_version,
        };
        let plaintext_len =
            i64::try_from(plaintext.len()).context("Gemini Batch chunk too large")?;
        let plaintext_digest = Sha256::digest(plaintext).into();
        let (key_id, nonce, ciphertext) = self.keyring.encrypt(identity.aad()?, plaintext)?;
        let chunk = GeminiBatchFileChunk {
            chunk_index: self.next_chunk_index,
            key_id,
            nonce: nonce.to_vec(),
            ciphertext,
            plaintext_len,
            plaintext_digest,
            created_ts,
        };
        chunk.validate()?;
        self.whole_file.update(plaintext);
        self.manifest_entries
            .push((chunk.chunk_index, plaintext_len, plaintext_digest));
        self.next_chunk_index = self
            .next_chunk_index
            .checked_add(1)
            .context("Gemini Batch chunk index overflow")?;
        Ok(chunk)
    }

    pub fn finish(self, completed_ts: i64) -> Result<GeminiBatchFileCompletion> {
        if completed_ts <= 0 {
            bail!("invalid Gemini Batch file completion timestamp")
        }
        Ok(GeminiBatchFileCompletion {
            completed_ts,
            whole_file_sha256_digest: self.whole_file.finalize().into(),
            chunk_manifest_digest: chunk_manifest_digest_from_entries(&self.manifest_entries)?,
        })
    }
}

/// Recompute the registry-compatible digest of an ordered durable chunk manifest.
pub fn gemini_batch_chunk_manifest_digest(chunks: &[GeminiBatchFileChunk]) -> Result<[u8; 32]> {
    let mut entries = Vec::with_capacity(chunks.len());
    for (expected, chunk) in chunks.iter().enumerate() {
        chunk.validate()?;
        if chunk.chunk_index
            != i64::try_from(expected).context("Gemini Batch chunk index overflow")?
        {
            bail!("Gemini Batch file chunks are not contiguous")
        }
        entries.push((
            chunk.chunk_index,
            chunk.plaintext_len,
            chunk.plaintext_digest,
        ));
    }
    chunk_manifest_digest_from_entries(&entries)
}

fn chunk_manifest_digest_from_entries(entries: &[(i64, i64, [u8; 32])]) -> Result<[u8; 32]> {
    let mut manifest = Sha256::new();
    manifest.update(FILE_CHUNK_MANIFEST_DOMAIN);
    manifest.update(
        u64::try_from(entries.len())
            .context("Gemini Batch chunk count overflow")?
            .to_be_bytes(),
    );
    for (index, len, digest) in entries {
        manifest.update(index.to_be_bytes());
        manifest.update(len.to_be_bytes());
        manifest.update(digest);
    }
    Ok(manifest.finalize().into())
}

fn verify_plaintext(plaintext: &[u8], expected_len: i64, expected_digest: &[u8; 32]) -> Result<()> {
    if i64::try_from(plaintext.len()).ok() != Some(expected_len)
        || Sha256::digest(plaintext).as_slice() != expected_digest
    {
        bail!("Gemini Batch plaintext integrity mismatch")
    }
    Ok(())
}

fn validate_component(value: &str, description: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > MAX_KEY_ID_LEN
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b';' || byte == b',' || byte == b':')
    {
        bail!("invalid Gemini Batch {description}")
    }
    Ok(())
}

fn validate_kind(kind: &str) -> Result<()> {
    if !matches!(kind, "request" | "metadata" | "result" | "error") {
        bail!("invalid Gemini Batch blob kind")
    }
    Ok(())
}

fn encode_field(output: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    output.extend_from_slice(
        &u32::try_from(value.len())
            .context("Gemini Batch AAD field too large")?
            .to_be_bytes(),
    );
    output.extend_from_slice(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> String {
        URL_SAFE_NO_PAD.encode([byte; 32])
    }

    fn ring(active: &str) -> GeminiBatchDataKeyring {
        GeminiBatchDataKeyring::parse(&format!("{active};new:{},old:{}", key(0x11), key(0x22)))
            .unwrap()
    }

    fn blob_identity<'a>(account: &'a str, job: &'a str) -> GeminiBatchBlobIdentity<'a> {
        GeminiBatchBlobIdentity {
            account_id: account,
            job_id: job,
            item_index: 3,
            kind: "request",
            schema_version: 1,
        }
    }

    #[test]
    fn rotation_reads_old_and_writes_active() {
        let old_ring = ring("old");
        let identity = blob_identity("account-a", "job-a");
        let old = old_ring
            .encrypt_blob(&identity, b"old secret", 100)
            .unwrap();
        assert_eq!(old.key_id, "old");

        let new_ring = ring("new");
        assert_eq!(
            &*new_ring.decrypt_blob(&identity, &old).unwrap(),
            b"old secret"
        );
        let new = new_ring
            .encrypt_blob(&identity, b"new secret", 100)
            .unwrap();
        assert_eq!(new.key_id, "new");

        let only_new = GeminiBatchDataKeyring::parse(&format!("new;new:{}", key(0x11))).unwrap();
        assert!(only_new.decrypt_blob(&identity, &old).is_err());
    }

    #[test]
    fn tamper_and_identity_swaps_fail_closed() {
        let ring = ring("new");
        let identity = blob_identity("account-a", "job-a");
        let blob = ring
            .encrypt_blob(&identity, b"customer plaintext", 100)
            .unwrap();

        let mut tampered = blob.clone();
        tampered.ciphertext[0] ^= 1;
        assert!(ring.decrypt_blob(&identity, &tampered).is_err());
        assert!(ring
            .decrypt_blob(&blob_identity("account-b", "job-a"), &blob)
            .is_err());
        assert!(ring
            .decrypt_blob(&blob_identity("account-a", "job-b"), &blob)
            .is_err());
        let mut wrong_item = identity;
        wrong_item.item_index += 1;
        assert!(ring.decrypt_blob(&wrong_item, &blob).is_err());
        let mut wrong_schema = identity;
        wrong_schema.schema_version += 1;
        assert!(ring.decrypt_blob(&wrong_schema, &blob).is_err());
        let mut wrong_kind = identity;
        wrong_kind.kind = "result";
        assert!(ring.decrypt_blob(&wrong_kind, &blob).is_err());
    }

    #[test]
    fn file_streaming_digests_match_registry_contract_and_bind_order() {
        let ring = ring("new");
        let mut encryptor = ring.file_encryptor("account-a", "file-a", 1).unwrap();
        let first = encryptor.push_chunk(b"hello ", 10).unwrap();
        let second = encryptor.push_chunk(b"world", 11).unwrap();
        let completion = encryptor.finish(12).unwrap();
        let chunks = vec![first, second];

        assert_eq!(
            completion.whole_file_sha256_digest,
            <[u8; 32]>::from(Sha256::digest(b"hello world"))
        );
        assert_eq!(
            completion.chunk_manifest_digest,
            gemini_batch_chunk_manifest_digest(&chunks).unwrap()
        );

        let first_identity = GeminiBatchFileChunkIdentity {
            account_id: "account-a",
            file_id: "file-a",
            chunk_index: 0,
            schema_version: 1,
        };
        assert_eq!(
            &*ring
                .decrypt_file_chunk(&first_identity, &chunks[0])
                .unwrap(),
            b"hello "
        );
        let mut wrong_file = first_identity;
        wrong_file.file_id = "file-b";
        assert!(ring.decrypt_file_chunk(&wrong_file, &chunks[0]).is_err());

        let mut swapped = chunks.clone();
        swapped.swap(0, 1);
        assert!(gemini_batch_chunk_manifest_digest(&swapped).is_err());
        let mut digest_tamper = chunks.clone();
        digest_tamper[0].plaintext_digest[0] ^= 1;
        assert_ne!(
            completion.chunk_manifest_digest,
            gemini_batch_chunk_manifest_digest(&digest_tamper).unwrap()
        );
    }

    #[test]
    fn empty_file_manifest_matches_registry_contract() {
        let completion = ring("new")
            .file_encryptor("account-a", "file-a", 1)
            .unwrap()
            .finish(10)
            .unwrap();
        assert_eq!(
            completion.whole_file_sha256_digest,
            <[u8; 32]>::from(Sha256::digest([]))
        );
        assert_eq!(
            completion.chunk_manifest_digest,
            [
                160, 21, 227, 48, 235, 217, 253, 190, 242, 13, 102, 210, 42, 9, 140, 207, 75, 199,
                12, 236, 163, 222, 104, 168, 110, 208, 96, 24, 193, 96, 120, 238
            ]
        );
    }

    #[test]
    fn parsing_and_debug_are_fail_closed_and_redacted() {
        for bad in [
            "",
            "new",
            "new;",
            "missing;new:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
            "new;new:short",
            "new;new:not-base64!",
        ] {
            assert!(
                GeminiBatchDataKeyring::parse(bad).is_err(),
                "accepted {bad}"
            );
        }
        let secret = key(0x7a);
        let ring = GeminiBatchDataKeyring::parse(&format!("private;private:{secret}")).unwrap();
        let identity = blob_identity("private-account", "private-job");
        let plaintext = "private-customer-payload";
        let blob = ring
            .encrypt_blob(&identity, plaintext.as_bytes(), 100)
            .unwrap();
        let combined = format!("{ring:?} {identity:?} {blob:?}");
        assert!(!combined.contains(&secret));
        assert!(!combined.contains("private-account"));
        assert!(!combined.contains("private-job"));
        assert!(!combined.contains(plaintext));
        assert!(!combined.contains("private-account"));
        assert!(!combined.contains("private-job"));
    }
}
