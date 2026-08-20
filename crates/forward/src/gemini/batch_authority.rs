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
    AcquireLeader {
        ttl_secs: i64,
        reply: Reply<bool>,
    },
    Claim {
        profile_id: String,
        model_id: String,
        lease_secs: i64,
        reply: Reply<Option<GeminiBatchClaimedItem>>,
    },
    MarkDispatching {
        claim: GeminiBatchClaim,
        lease_secs: i64,
        reply: Reply<bool>,
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
                    match command {
                        Command::AcquireLeader { ttl_secs, reply } => {
                            let _ =
                                reply.send(authority.acquire_gemini_batch_leader(&owner, ttl_secs));
                        }
                        Command::Claim {
                            profile_id,
                            model_id,
                            lease_secs,
                            reply,
                        } => {
                            let _ = reply.send(authority.claim_gemini_batch_item(
                                &owner,
                                &profile_id,
                                &model_id,
                                lease_secs,
                            ));
                        }
                        Command::MarkDispatching {
                            claim,
                            lease_secs,
                            reply,
                        } => {
                            let _ = reply.send(
                                authority.mark_gemini_batch_dispatching(&owner, &claim, lease_secs),
                            );
                        }
                        Command::MarkActualSend {
                            claim,
                            lease_secs,
                            reply,
                        } => {
                            let _ = reply.send(
                                authority.mark_gemini_batch_actual_send(&owner, &claim, lease_secs),
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
                            let _ =
                                reply.send(authority.reconcile_expired_gemini_batch_claims(limit));
                        }
                        Command::EnqueueLiveSettlement {
                            claim,
                            intent,
                            reply,
                        } => {
                            let _ = reply.send(
                                authority.enqueue_gemini_batch_settlement(&owner, &claim, &intent),
                            );
                        }
                        Command::EnqueueRecoverySettlement {
                            recovery,
                            intent,
                            reply,
                        } => {
                            let result = authority.postgres().and_then(|postgres| {
                                postgres
                                    .enqueue_gemini_batch_recovery_settlement(&recovery, &intent)
                            });
                            let _ = reply.send(result);
                        }
                        Command::ProcessSettlement { request_id, reply } => {
                            let _ =
                                reply.send(authority.process_gemini_batch_settlement(&request_id));
                        }
                        Command::DrainSettlements { limit, reply } => {
                            let _ = reply.send(authority.drain_gemini_batch_settlements(limit));
                        }
                        Command::Shutdown(reply) => {
                            let _ = reply.send(Ok(()));
                            break;
                        }
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
            .map_err(|_| anyhow::anyhow!("Gemini Batch authority stopped"))?
    }

    pub async fn acquire_leader(&self, ttl_secs: i64) -> Result<bool> {
        self.call(|reply| Command::AcquireLeader { ttl_secs, reply })
            .await
    }

    pub async fn claim(
        &self,
        profile_id: impl Into<String>,
        model_id: impl Into<String>,
        lease_secs: i64,
    ) -> Result<Option<GeminiBatchClaimedItem>> {
        self.call(|reply| Command::Claim {
            profile_id: profile_id.into(),
            model_id: model_id.into(),
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
            "Gemini Batch authority unavailable" | "Gemini Batch authority stopped"
        ));
    }
}
