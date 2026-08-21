//! Dedicated bounded sync-DB actor for Gemini Batch dispatch and settlement work.

use anyhow::Result;
use registry::{
    authority::AuthorityConfig,
    pg::{GeminiBatchReconcileReport, Owner},
    GeminiBatchClaim, GeminiBatchClaimedItem, GeminiBatchRecoveryCandidate,
    GeminiBatchSettlementIntent,
};
use tokio::sync::{mpsc, oneshot};

const COMMAND_CAPACITY: usize = 256;
const APPLICATION_NAME: &str = "gemini-batch-authority";

type Reply<T> = oneshot::Sender<Result<T>>;

enum Command {
    Create {
        create: registry::GeminiBatchCreate,
        raw_key: String,
        reply: Reply<registry::GeminiBatchCreateOutcome>,
    },
    Get {
        account_id: String,
        job_id: String,
        reply: Reply<Option<registry::GeminiBatchJobDetail>>,
    },
    List {
        account_id: String,
        cursor: Option<registry::GeminiBatchPageCursor>,
        limit: i64,
        reply: Reply<registry::GeminiBatchJobPage>,
    },
    Cancel {
        account_id: String,
        job_id: String,
        reply: Reply<Option<registry::GeminiBatchCancelResult>>,
    },
    Delete {
        account_id: String,
        job_id: String,
        reply: Reply<bool>,
    },
    FileCreate {
        create: registry::GeminiBatchFileCreate,
        reply: Reply<registry::GeminiBatchFileCreateOutcome>,
    },
    FileAppend {
        account_id: String,
        file_id: String,
        expected_offset: i64,
        chunk: registry::GeminiBatchFileChunk,
        reply: Reply<registry::GeminiBatchFileAppendOutcome>,
    },
    FileProgress {
        account_id: String,
        file_id: String,
        reply: Reply<Option<registry::GeminiBatchFileProgress>>,
    },
    FileComplete {
        account_id: String,
        file_id: String,
        completion: registry::GeminiBatchFileCompletion,
        reply: Reply<bool>,
    },
    FileGet {
        account_id: String,
        file_id: String,
        reply: Reply<Option<registry::GeminiBatchFile>>,
    },
    FileList {
        account_id: String,
        limit: i64,
        reply: Reply<Vec<registry::GeminiBatchFile>>,
    },
    FileDelete {
        account_id: String,
        file_id: String,
        reply: Reply<bool>,
    },
    FileChunks {
        account_id: String,
        file_id: String,
        after: Option<i64>,
        limit: i64,
        reply: Reply<registry::GeminiBatchFileChunkPage>,
    },
    BlobGet {
        account_id: String,
        job_id: String,
        item_index: i64,
        kind: String,
        reply: Reply<Option<registry::GeminiBatchEncryptedBlob>>,
    },
    AcquireLeader {
        ttl_secs: i64,
        reply: Reply<bool>,
    },
    Claim {
        profile_id: String,
        model_id: String,
        profile_capacity: i16,
        lease_secs: i64,
        reply: Reply<Option<GeminiBatchClaimedItem>>,
    },
    MarkDispatching {
        claim: GeminiBatchClaim,
        lease_secs: i64,
        reply: Reply<bool>,
    },
    ReserveDispatch {
        claim: GeminiBatchClaim,
        random_delay_ms: i64,
        reply: Reply<registry::GeminiBatchDispatchReservation>,
    },
    MarkActualSend {
        claim: GeminiBatchClaim,
        lease_secs: i64,
        reply: Reply<bool>,
    },
    Renew {
        claim: GeminiBatchClaim,
        lease_secs: i64,
        reply: Reply<bool>,
    },
    Requeue {
        claim: GeminiBatchClaim,
        next_attempt_ts: i64,
        reply: Reply<bool>,
    },
    Reconcile {
        limit: usize,
        reply: Reply<GeminiBatchReconcileReport>,
    },
    EnqueueLiveSettlement {
        claim: GeminiBatchClaim,
        intent: GeminiBatchSettlementIntent,
        reply: Reply<()>,
    },
    EnqueueRecoverySettlement {
        recovery: GeminiBatchRecoveryCandidate,
        intent: GeminiBatchSettlementIntent,
        reply: Reply<()>,
    },
    ProcessSettlement {
        request_id: String,
        reply: Reply<Option<i64>>,
    },
    DrainSettlements {
        limit: usize,
        reply: Reply<usize>,
    },
    ClaimOutput {
        lease_secs: i64,
        reply: Reply<Option<registry::GeminiBatchOutputClaim>>,
    },
    RenewOutput {
        claim: registry::GeminiBatchOutputClaim,
        lease_secs: i64,
        reply: Reply<bool>,
    },
    OutputItems {
        claim: registry::GeminiBatchOutputClaim,
        after: Option<i64>,
        limit: i64,
        reply: Reply<registry::GeminiBatchOutputItemPage>,
    },
    AppendOutput {
        claim: registry::GeminiBatchOutputClaim,
        next_item_index: i64,
        chunk: registry::GeminiBatchFileChunk,
        reply: Reply<bool>,
    },
    FailOutput {
        claim: registry::GeminiBatchOutputClaim,
        class: String,
        reply: Reply<bool>,
    },
    FinalizeOutput {
        claim: registry::GeminiBatchOutputClaim,
        completion: registry::GeminiBatchFileCompletion,
        reply: Reply<bool>,
    },
    Maintain {
        older_than: i64,
        limit: usize,
        reply: Reply<registry::GeminiBatchMaintenanceReport>,
    },
    OperationalReport(Reply<registry::GeminiBatchOperationalReport>),
    #[cfg(test)]
    Panic(Reply<()>),
    Shutdown(Reply<()>),
}

/// Async facade for one synchronous authority connection owned by one operating-system thread.
#[derive(Clone)]
pub struct GeminiBatchAuthority {
    commands: mpsc::Sender<Command>,
}

impl GeminiBatchAuthority {
    /// Start the bounded actor. The authority connects exactly once on its dedicated thread.
    pub fn start(config: AuthorityConfig, owner: Owner) -> Result<Self> {
        Self::start_with_capacity(config, owner, COMMAND_CAPACITY)
    }

    fn start_with_capacity(config: AuthorityConfig, owner: Owner, capacity: usize) -> Result<Self> {
        if capacity == 0 {
            anyhow::bail!("Gemini Batch authority capacity must be positive")
        }
        let (commands, mut receiver) = mpsc::channel(capacity);
        let (started, startup) = std::sync::mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name(APPLICATION_NAME.to_owned())
            .spawn(move || {
                let mut authority = match config.connect_with_application_name(APPLICATION_NAME) {
                    Ok(authority) => {
                        let _ = started.send(Ok(()));
                        authority
                    }
                    Err(error) => {
                        let _ = started.send(Err(error));
                        return;
                    }
                };
                while let Some(command) = receiver.blocking_recv() {
                    let command = match command {
                        Command::Shutdown(reply) => {
                            let _ = reply.send(Ok(()));
                            break;
                        }
                        command => command,
                    };
                    let outcome =
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match command {
                            Command::Create {
                                create,
                                raw_key,
                                reply,
                            } => {
                                let _ =
                                    reply.send(authority.gemini_batch_create(&create, &raw_key));
                            }
                            Command::Get {
                                account_id,
                                job_id,
                                reply,
                            } => {
                                let _ =
                                    reply.send(authority.gemini_batch_get(&account_id, &job_id));
                            }
                            Command::List {
                                account_id,
                                cursor,
                                limit,
                                reply,
                            } => {
                                let _ = reply.send(authority.gemini_batch_list(
                                    &account_id,
                                    cursor.as_ref(),
                                    limit,
                                ));
                            }
                            Command::Cancel {
                                account_id,
                                job_id,
                                reply,
                            } => {
                                let _ =
                                    reply.send(authority.gemini_batch_cancel(&account_id, &job_id));
                            }
                            Command::Delete {
                                account_id,
                                job_id,
                                reply,
                            } => {
                                let _ =
                                    reply.send(authority.gemini_batch_delete(&account_id, &job_id));
                            }
                            Command::FileCreate { create, reply } => {
                                let _ = reply.send(authority.gemini_batch_file_create(&create));
                            }
                            Command::FileAppend {
                                account_id,
                                file_id,
                                expected_offset,
                                chunk,
                                reply,
                            } => {
                                let _ = reply.send(authority.gemini_batch_file_append_chunk_at(
                                    &account_id,
                                    &file_id,
                                    expected_offset,
                                    &chunk,
                                ));
                            }
                            Command::FileProgress {
                                account_id,
                                file_id,
                                reply,
                            } => {
                                let _ = reply.send(
                                    authority.gemini_batch_file_progress(&account_id, &file_id),
                                );
                            }
                            Command::FileComplete {
                                account_id,
                                file_id,
                                completion,
                                reply,
                            } => {
                                let _ = reply.send(authority.gemini_batch_file_complete(
                                    &account_id,
                                    &file_id,
                                    &completion,
                                ));
                            }
                            Command::FileGet {
                                account_id,
                                file_id,
                                reply,
                            } => {
                                let _ = reply
                                    .send(authority.gemini_batch_file_get(&account_id, &file_id));
                            }
                            Command::FileList {
                                account_id,
                                limit,
                                reply,
                            } => {
                                let _ = reply
                                    .send(authority.gemini_batch_file_list(&account_id, limit));
                            }
                            Command::FileDelete {
                                account_id,
                                file_id,
                                reply,
                            } => {
                                let _ = reply.send(
                                    authority.gemini_batch_file_delete(&account_id, &file_id),
                                );
                            }
                            Command::FileChunks {
                                account_id,
                                file_id,
                                after,
                                limit,
                                reply,
                            } => {
                                let _ = reply.send(authority.gemini_batch_file_chunk_page(
                                    &account_id,
                                    &file_id,
                                    after,
                                    limit,
                                ));
                            }
                            Command::BlobGet {
                                account_id,
                                job_id,
                                item_index,
                                kind,
                                reply,
                            } => {
                                let _ = reply.send(authority.gemini_batch_blob_get(
                                    &account_id,
                                    &job_id,
                                    item_index,
                                    &kind,
                                ));
                            }
                            Command::AcquireLeader { ttl_secs, reply } => {
                                let _ = reply
                                    .send(authority.acquire_gemini_batch_leader(&owner, ttl_secs));
                            }
                            Command::Claim {
                                profile_id,
                                model_id,
                                profile_capacity,
                                lease_secs,
                                reply,
                            } => {
                                let _ = reply.send(authority.claim_gemini_batch_item(
                                    &owner,
                                    &profile_id,
                                    &model_id,
                                    profile_capacity,
                                    lease_secs,
                                ));
                            }
                            Command::MarkDispatching {
                                claim,
                                lease_secs,
                                reply,
                            } => {
                                let _ = reply.send(
                                    authority
                                        .mark_gemini_batch_dispatching(&owner, &claim, lease_secs),
                                );
                            }
                            Command::ReserveDispatch {
                                claim,
                                random_delay_ms,
                                reply,
                            } => {
                                let _ = reply.send(authority.reserve_gemini_batch_dispatch(
                                    &owner,
                                    &claim,
                                    random_delay_ms,
                                ));
                            }
                            Command::MarkActualSend {
                                claim,
                                lease_secs,
                                reply,
                            } => {
                                let _ = reply.send(
                                    authority
                                        .mark_gemini_batch_actual_send(&owner, &claim, lease_secs),
                                );
                            }
                            Command::Renew {
                                claim,
                                lease_secs,
                                reply,
                            } => {
                                let _ = reply.send(
                                    authority.renew_gemini_batch_claim(&owner, &claim, lease_secs),
                                );
                            }
                            Command::Requeue {
                                claim,
                                next_attempt_ts,
                                reply,
                            } => {
                                let _ = reply.send(authority.requeue_gemini_batch_claim(
                                    &owner,
                                    &claim,
                                    next_attempt_ts,
                                ));
                            }
                            Command::Reconcile { limit, reply } => {
                                let _ = reply
                                    .send(authority.reconcile_expired_gemini_batch_claims(limit));
                            }
                            Command::EnqueueLiveSettlement {
                                claim,
                                intent,
                                reply,
                            } => {
                                let _ = reply.send(
                                    authority
                                        .enqueue_gemini_batch_settlement(&owner, &claim, &intent),
                                );
                            }
                            Command::EnqueueRecoverySettlement {
                                recovery,
                                intent,
                                reply,
                            } => {
                                let result = authority.postgres().and_then(|postgres| {
                                    postgres.enqueue_gemini_batch_recovery_settlement(
                                        &recovery, &intent,
                                    )
                                });
                                let _ = reply.send(result);
                            }
                            Command::ProcessSettlement { request_id, reply } => {
                                let _ = reply
                                    .send(authority.process_gemini_batch_settlement(&request_id));
                            }
                            Command::DrainSettlements { limit, reply } => {
                                let _ = reply.send(authority.drain_gemini_batch_settlements(limit));
                            }
                            Command::ClaimOutput { lease_secs, reply } => {
                                let _ = reply
                                    .send(authority.claim_gemini_batch_output(&owner, lease_secs));
                            }
                            Command::RenewOutput {
                                claim,
                                lease_secs,
                                reply,
                            } => {
                                let _ = reply.send(
                                    authority.renew_gemini_batch_output(&owner, &claim, lease_secs),
                                );
                            }
                            Command::OutputItems {
                                claim,
                                after,
                                limit,
                                reply,
                            } => {
                                let _ =
                                    reply.send(authority.gemini_batch_output_item_page(
                                        &owner, &claim, after, limit,
                                    ));
                            }
                            Command::AppendOutput {
                                claim,
                                next_item_index,
                                chunk,
                                reply,
                            } => {
                                let _ = reply.send(authority.append_gemini_batch_output_chunk(
                                    &owner,
                                    &claim,
                                    next_item_index,
                                    &chunk,
                                ));
                            }
                            Command::FailOutput {
                                claim,
                                class,
                                reply,
                            } => {
                                let _ = reply.send(
                                    authority.fail_gemini_batch_output(&owner, &claim, &class),
                                );
                            }
                            Command::FinalizeOutput {
                                claim,
                                completion,
                                reply,
                            } => {
                                let _ = reply.send(authority.finalize_gemini_batch_output(
                                    &owner,
                                    &claim,
                                    &completion,
                                ));
                            }
                            Command::Maintain {
                                older_than,
                                limit,
                                reply,
                            } => {
                                let _ =
                                    reply.send(authority.maintain_gemini_batch(older_than, limit));
                            }
                            Command::OperationalReport(reply) => {
                                let _ = reply.send(authority.gemini_batch_operational_report());
                            }
                            #[cfg(test)]
                            Command::Panic(_reply) => panic!("Gemini Batch authority test panic"),
                            Command::Shutdown(_) => {
                                unreachable!("shutdown handled before command execution")
                            }
                        }));
                    if outcome.is_err() {
                        elog::error(
                            "gemini-batch-authority",
                            "Gemini Batch authority command panicked; actor remains available",
                        );
                    }
                }
            })?;
        startup.recv().map_err(|_| {
            anyhow::anyhow!("Gemini Batch authority thread stopped during startup")
        })??;
        Ok(Self { commands })
    }

    async fn call<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T> {
        let (reply, result) = oneshot::channel();
        self.commands
            .send(make(reply))
            .await
            .map_err(|_| anyhow::anyhow!("Gemini Batch authority unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("Gemini Batch authority command aborted"))?
    }

    pub async fn create(
        &self,
        create: registry::GeminiBatchCreate,
        raw_key: String,
    ) -> Result<registry::GeminiBatchCreateOutcome> {
        self.call(|reply| Command::Create {
            create,
            raw_key,
            reply,
        })
        .await
    }

    pub async fn get(
        &self,
        account_id: String,
        job_id: String,
    ) -> Result<Option<registry::GeminiBatchJobDetail>> {
        self.call(|reply| Command::Get {
            account_id,
            job_id,
            reply,
        })
        .await
    }

    pub async fn list(
        &self,
        account_id: String,
        cursor: Option<registry::GeminiBatchPageCursor>,
        limit: i64,
    ) -> Result<registry::GeminiBatchJobPage> {
        self.call(|reply| Command::List {
            account_id,
            cursor,
            limit,
            reply,
        })
        .await
    }

    pub async fn cancel(
        &self,
        account_id: String,
        job_id: String,
    ) -> Result<Option<registry::GeminiBatchCancelResult>> {
        self.call(|reply| Command::Cancel {
            account_id,
            job_id,
            reply,
        })
        .await
    }

    pub async fn delete(&self, account_id: String, job_id: String) -> Result<bool> {
        self.call(|reply| Command::Delete {
            account_id,
            job_id,
            reply,
        })
        .await
    }

    pub async fn file_create(
        &self,
        create: registry::GeminiBatchFileCreate,
    ) -> Result<registry::GeminiBatchFileCreateOutcome> {
        self.call(|reply| Command::FileCreate { create, reply })
            .await
    }
    pub async fn file_progress(
        &self,
        account_id: String,
        file_id: String,
    ) -> Result<Option<registry::GeminiBatchFileProgress>> {
        self.call(|reply| Command::FileProgress {
            account_id,
            file_id,
            reply,
        })
        .await
    }
    pub async fn file_append(
        &self,
        account_id: String,
        file_id: String,
        expected_offset: i64,
        chunk: registry::GeminiBatchFileChunk,
    ) -> Result<registry::GeminiBatchFileAppendOutcome> {
        self.call(|reply| Command::FileAppend {
            account_id,
            file_id,
            expected_offset,
            chunk,
            reply,
        })
        .await
    }
    pub async fn file_complete(
        &self,
        account_id: String,
        file_id: String,
        completion: registry::GeminiBatchFileCompletion,
    ) -> Result<bool> {
        self.call(|reply| Command::FileComplete {
            account_id,
            file_id,
            completion,
            reply,
        })
        .await
    }
    pub async fn file_get(
        &self,
        account_id: String,
        file_id: String,
    ) -> Result<Option<registry::GeminiBatchFile>> {
        self.call(|reply| Command::FileGet {
            account_id,
            file_id,
            reply,
        })
        .await
    }
    pub async fn file_list(
        &self,
        account_id: String,
        limit: i64,
    ) -> Result<Vec<registry::GeminiBatchFile>> {
        self.call(|reply| Command::FileList {
            account_id,
            limit,
            reply,
        })
        .await
    }
    pub async fn file_delete(&self, account_id: String, file_id: String) -> Result<bool> {
        self.call(|reply| Command::FileDelete {
            account_id,
            file_id,
            reply,
        })
        .await
    }
    pub async fn file_chunks(
        &self,
        account_id: String,
        file_id: String,
        after: Option<i64>,
        limit: i64,
    ) -> Result<registry::GeminiBatchFileChunkPage> {
        self.call(|reply| Command::FileChunks {
            account_id,
            file_id,
            after,
            limit,
            reply,
        })
        .await
    }
    pub async fn blob_get(
        &self,
        account_id: String,
        job_id: String,
        item_index: i64,
        kind: String,
    ) -> Result<Option<registry::GeminiBatchEncryptedBlob>> {
        self.call(|reply| Command::BlobGet {
            account_id,
            job_id,
            item_index,
            kind,
            reply,
        })
        .await
    }

    pub async fn acquire_leader(&self, ttl_secs: i64) -> Result<bool> {
        self.call(|reply| Command::AcquireLeader { ttl_secs, reply })
            .await
    }

    pub async fn claim(
        &self,
        profile_id: impl Into<String>,
        model_id: impl Into<String>,
        profile_capacity: i16,
        lease_secs: i64,
    ) -> Result<Option<GeminiBatchClaimedItem>> {
        self.call(|reply| Command::Claim {
            profile_id: profile_id.into(),
            model_id: model_id.into(),
            profile_capacity,
            lease_secs,
            reply,
        })
        .await
    }

    pub async fn mark_dispatching(&self, claim: GeminiBatchClaim, lease_secs: i64) -> Result<bool> {
        self.call(|reply| Command::MarkDispatching {
            claim,
            lease_secs,
            reply,
        })
        .await
    }

    pub async fn reserve_dispatch(
        &self,
        claim: GeminiBatchClaim,
        random_delay_ms: i64,
    ) -> Result<registry::GeminiBatchDispatchReservation> {
        self.call(|reply| Command::ReserveDispatch {
            claim,
            random_delay_ms,
            reply,
        })
        .await
    }

    pub async fn mark_actual_send(&self, claim: GeminiBatchClaim, lease_secs: i64) -> Result<bool> {
        self.call(|reply| Command::MarkActualSend {
            claim,
            lease_secs,
            reply,
        })
        .await
    }

    pub async fn renew(&self, claim: GeminiBatchClaim, lease_secs: i64) -> Result<bool> {
        self.call(|reply| Command::Renew {
            claim,
            lease_secs,
            reply,
        })
        .await
    }

    pub async fn requeue(&self, claim: GeminiBatchClaim, next_attempt_ts: i64) -> Result<bool> {
        self.call(|reply| Command::Requeue {
            claim,
            next_attempt_ts,
            reply,
        })
        .await
    }

    pub async fn reconcile(&self, limit: usize) -> Result<GeminiBatchReconcileReport> {
        self.call(|reply| Command::Reconcile { limit, reply }).await
    }

    pub async fn enqueue_live_settlement(
        &self,
        claim: GeminiBatchClaim,
        intent: GeminiBatchSettlementIntent,
    ) -> Result<()> {
        self.call(|reply| Command::EnqueueLiveSettlement {
            claim,
            intent,
            reply,
        })
        .await
    }

    pub async fn enqueue_recovery_settlement(
        &self,
        recovery: GeminiBatchRecoveryCandidate,
        intent: GeminiBatchSettlementIntent,
    ) -> Result<()> {
        self.call(|reply| Command::EnqueueRecoverySettlement {
            recovery,
            intent,
            reply,
        })
        .await
    }

    pub async fn process_settlement(&self, request_id: impl Into<String>) -> Result<Option<i64>> {
        self.call(|reply| Command::ProcessSettlement {
            request_id: request_id.into(),
            reply,
        })
        .await
    }

    pub async fn drain_settlements(&self, limit: usize) -> Result<usize> {
        self.call(|reply| Command::DrainSettlements { limit, reply })
            .await
    }

    pub async fn claim_output(
        &self,
        lease_secs: i64,
    ) -> Result<Option<registry::GeminiBatchOutputClaim>> {
        self.call(|reply| Command::ClaimOutput { lease_secs, reply })
            .await
    }
    pub async fn renew_output(
        &self,
        claim: registry::GeminiBatchOutputClaim,
        lease_secs: i64,
    ) -> Result<bool> {
        self.call(|reply| Command::RenewOutput {
            claim,
            lease_secs,
            reply,
        })
        .await
    }
    pub async fn output_items(
        &self,
        claim: registry::GeminiBatchOutputClaim,
        after: Option<i64>,
        limit: i64,
    ) -> Result<registry::GeminiBatchOutputItemPage> {
        self.call(|reply| Command::OutputItems {
            claim,
            after,
            limit,
            reply,
        })
        .await
    }
    pub async fn append_output(
        &self,
        claim: registry::GeminiBatchOutputClaim,
        next_item_index: i64,
        chunk: registry::GeminiBatchFileChunk,
    ) -> Result<bool> {
        self.call(|reply| Command::AppendOutput {
            claim,
            next_item_index,
            chunk,
            reply,
        })
        .await
    }
    pub async fn fail_output(
        &self,
        claim: registry::GeminiBatchOutputClaim,
        class: impl Into<String>,
    ) -> Result<bool> {
        self.call(|reply| Command::FailOutput {
            claim,
            class: class.into(),
            reply,
        })
        .await
    }
    pub async fn finalize_output(
        &self,
        claim: registry::GeminiBatchOutputClaim,
        completion: registry::GeminiBatchFileCompletion,
    ) -> Result<bool> {
        self.call(|reply| Command::FinalizeOutput {
            claim,
            completion,
            reply,
        })
        .await
    }
    pub async fn maintain(
        &self,
        older_than: i64,
        limit: usize,
    ) -> Result<registry::GeminiBatchMaintenanceReport> {
        self.call(|reply| Command::Maintain {
            older_than,
            limit,
            reply,
        })
        .await
    }

    pub async fn operational_report(&self) -> Result<registry::GeminiBatchOperationalReport> {
        self.call(Command::OperationalReport).await
    }

    /// FIFO barrier: success means every command submitted before shutdown finished processing.
    pub async fn shutdown(&self) -> Result<()> {
        self.call(Command::Shutdown).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sqlite_actor(capacity: usize) -> GeminiBatchAuthority {
        GeminiBatchAuthority::start_with_capacity(
            AuthorityConfig::Sqlite {
                path: ":memory:".to_owned(),
            },
            Owner {
                instance_id: "batch-test".to_owned(),
                epoch: 1,
            },
            capacity,
        )
        .unwrap()
    }

    #[tokio::test]
    async fn sqlite_returns_typed_unsupported_from_authority() {
        let actor = sqlite_actor(4);
        let error = actor.acquire_leader(30).await.unwrap_err();
        assert!(registry::is_gemini_batch_unsupported(&error));
        actor.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn a_panicking_command_does_not_kill_the_authority_actor() {
        let actor = sqlite_actor(4);
        let error = actor.call(Command::Panic).await.unwrap_err();
        assert_eq!(error.to_string(), "Gemini Batch authority command aborted");

        let error = actor.operational_report().await.unwrap_err();
        assert!(registry::is_gemini_batch_unsupported(&error));
        actor.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_is_a_fifo_barrier() {
        let actor = sqlite_actor(4);
        let (reply, first) = oneshot::channel();
        actor
            .commands
            .send(Command::AcquireLeader {
                ttl_secs: 30,
                reply,
            })
            .await
            .unwrap();

        actor.shutdown().await.unwrap();
        let error = first.await.unwrap().unwrap_err();
        assert!(registry::is_gemini_batch_unsupported(&error));
        let after = actor.acquire_leader(30).await.unwrap_err();
        assert!(matches!(
            after.to_string().as_str(),
            "Gemini Batch authority unavailable" | "Gemini Batch authority command aborted"
        ));
    }
}
