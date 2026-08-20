//! Dedicated bounded schema-60 admission actor.
//!
//! It owns a separate synchronous authority connection so staging and the final set-based publish
//! cannot delay scheduler leases or settlement on the operational Batch actor.

use anyhow::Result;
use registry::{
    authority::AuthorityConfig, GeminiBatchAdmissionBegin, GeminiBatchAdmissionBeginOutcome,
    GeminiBatchAdmissionItem, GeminiBatchCreateOutcome,
};
use tokio::sync::{mpsc, oneshot};

const COMMAND_CAPACITY: usize = 32;
const APPLICATION_NAME: &str = "gemini-batch-ingest";
type Reply<T> = oneshot::Sender<Result<T>>;

enum Command {
    Begin(
        GeminiBatchAdmissionBegin,
        Reply<GeminiBatchAdmissionBeginOutcome>,
    ),
    Append {
        admission_id: String,
        expected_start: i64,
        items: Vec<GeminiBatchAdmissionItem>,
        reply: Reply<i64>,
    },
    Publish {
        admission_id: String,
        expected_items: i64,
        canonical_request_digest: [u8; 32],
        raw_key: String,
        reply: Reply<GeminiBatchCreateOutcome>,
    },
    Abort(String, Reply<bool>),
}

#[derive(Clone)]
pub struct GeminiBatchIngest {
    commands: mpsc::Sender<Command>,
}

impl GeminiBatchIngest {
    pub fn start(config: AuthorityConfig) -> Result<Self> {
        let (commands, mut receiver) = mpsc::channel(COMMAND_CAPACITY);
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
                        Command::Begin(begin, reply) => {
                            let _ = reply.send(authority.gemini_batch_admission_begin(&begin));
                        }
                        Command::Append {
                            admission_id,
                            expected_start,
                            items,
                            reply,
                        } => {
                            let _ = reply.send(authority.gemini_batch_admission_append(
                                &admission_id,
                                expected_start,
                                &items,
                            ));
                        }
                        Command::Publish {
                            admission_id,
                            expected_items,
                            canonical_request_digest,
                            raw_key,
                            reply,
                        } => {
                            let _ = reply.send(authority.gemini_batch_admission_publish(
                                &admission_id,
                                expected_items,
                                canonical_request_digest,
                                &raw_key,
                            ));
                        }
                        Command::Abort(admission_id, reply) => {
                            let _ =
                                reply.send(authority.gemini_batch_admission_abort(&admission_id));
                        }
                    }
                }
            })?;
        startup
            .recv()
            .map_err(|_| anyhow::anyhow!("Gemini Batch ingest thread stopped during startup"))??;
        Ok(Self { commands })
    }

    async fn call<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T> {
        let (reply, result) = oneshot::channel();
        self.commands
            .send(make(reply))
            .await
            .map_err(|_| anyhow::anyhow!("Gemini Batch ingest unavailable"))?;
        result
            .await
            .map_err(|_| anyhow::anyhow!("Gemini Batch ingest stopped"))?
    }

    pub async fn begin(
        &self,
        begin: GeminiBatchAdmissionBegin,
    ) -> Result<GeminiBatchAdmissionBeginOutcome> {
        self.call(|reply| Command::Begin(begin, reply)).await
    }

    pub async fn append(
        &self,
        admission_id: String,
        expected_start: i64,
        items: Vec<GeminiBatchAdmissionItem>,
    ) -> Result<i64> {
        self.call(|reply| Command::Append {
            admission_id,
            expected_start,
            items,
            reply,
        })
        .await
    }

    pub async fn publish(
        &self,
        admission_id: String,
        expected_items: i64,
        canonical_request_digest: [u8; 32],
        raw_key: String,
    ) -> Result<GeminiBatchCreateOutcome> {
        self.call(|reply| Command::Publish {
            admission_id,
            expected_items,
            canonical_request_digest,
            raw_key,
            reply,
        })
        .await
    }

    pub async fn abort(&self, admission_id: String) -> Result<bool> {
        self.call(|reply| Command::Abort(admission_id, reply)).await
    }
}
