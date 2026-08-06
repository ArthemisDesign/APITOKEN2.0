use super::*;
use futures_util::StreamExt;
use std::sync::atomic::Ordering;

fn body_of(chunks: Vec<Result<bytes::Bytes, std::io::Error>>) -> ResponseByteStream {
    Box::pin(futures_util::stream::iter(chunks))
}

fn metrics() -> Arc<Metrics> {
    Arc::new(Metrics::default())
}

#[tokio::test]
async fn a_broken_stream_ends_with_the_protocol_error_frame() {
    // Without this the body just stops. An SDK cannot tell a truncated stream from a finished
    // one until something further up fails to parse, and many clients wait forever instead.
    let inner = body_of(vec![
        Ok(bytes::Bytes::from_static(b"event: message_start\n\n")),
        Err(std::io::Error::other("upstream vanished")),
    ]);
    let collected: Vec<_> = SseErrorTail::new(inner, metrics()).collect().await;
    assert_eq!(collected.len(), 2);
    let tail = collected[1].as_ref().expect("tail frame is not an error");
    let tail = String::from_utf8(tail.to_vec()).unwrap();
    assert!(tail.starts_with("event: error\n"), "{tail}");
    assert!(tail.contains("\"type\":\"error\""), "{tail}");
    // The cause belongs in metrics and logs; the customer gets the anonymised overload wording.
    assert!(!tail.contains("upstream vanished"), "{tail}");
    assert!(tail.ends_with("\n\n"), "{tail}");
}

#[tokio::test]
async fn a_clean_stream_is_passed_through_untouched() {
    let inner = body_of(vec![
        Ok(bytes::Bytes::from_static(b"event: message_start\n\n")),
        Ok(bytes::Bytes::from_static(b"event: message_stop\n\n")),
    ]);
    let collected: Vec<_> = SseErrorTail::new(inner, metrics()).collect().await;
    assert_eq!(collected.len(), 2);
    assert!(collected.iter().all(|chunk| chunk.is_ok()));
    let joined: Vec<u8> = collected
        .into_iter()
        .flat_map(|chunk| chunk.unwrap().to_vec())
        .collect();
    // A successful stream must be byte-for-byte what the upstream sent.
    assert_eq!(
        String::from_utf8(joined).unwrap(),
        "event: message_start\n\nevent: message_stop\n\n"
    );
}

#[tokio::test]
async fn the_error_frame_is_emitted_exactly_once() {
    let inner = body_of(vec![
        Err(std::io::Error::other("first")),
        Ok(bytes::Bytes::from_static(b"never reached")),
    ]);
    let collected: Vec<_> = SseErrorTail::new(inner, metrics()).collect().await;
    assert_eq!(collected.len(), 1);
    assert!(String::from_utf8(collected[0].as_ref().unwrap().to_vec())
        .unwrap()
        .starts_with("event: error\n"));
}

/// A cut stream must leave a trace, and the trace must name the cause.
///
/// The customer already received `200`, so the terminal-error audit never sees this failure. When
/// the branch also counted nothing, "the connection keeps dropping" was unfalsifiable: the client
/// showed `overloaded_error` while every counter and log on our side stayed clean. An idle upstream
/// and a proxy that cut the tunnel produce the identical customer frame, so they are separated
/// here — that split is the whole diagnostic value.
#[tokio::test]
async fn a_cut_stream_is_counted_by_cause() {
    let transport = metrics();
    let inner = body_of(vec![
        Ok(bytes::Bytes::from_static(b"event: message_start\n\n")),
        Err(std::io::Error::other("tunnel closed")),
    ]);
    let _: Vec<_> = SseErrorTail::new(inner, transport.clone()).collect().await;
    assert_eq!(
        transport.stream_cut_transport.load(Ordering::Relaxed),
        1,
        "a transport failure must be counted as transport"
    );
    assert_eq!(transport.stream_cut_timeout.load(Ordering::Relaxed), 0);

    let idle = metrics();
    let inner = body_of(vec![
        Ok(bytes::Bytes::from_static(b"event: message_start\n\n")),
        Err(std::io::Error::from(std::io::ErrorKind::TimedOut)),
    ]);
    let _: Vec<_> = SseErrorTail::new(inner, idle.clone()).collect().await;
    assert_eq!(
        idle.stream_cut_timeout.load(Ordering::Relaxed),
        1,
        "an idle-timeout cut must be counted separately from a transport one"
    );
    assert_eq!(idle.stream_cut_transport.load(Ordering::Relaxed), 0);

    let clean = metrics();
    let inner = body_of(vec![Ok(bytes::Bytes::from_static(b"event: message_stop\n\n"))]);
    let _: Vec<_> = SseErrorTail::new(inner, clean.clone()).collect().await;
    assert_eq!(clean.stream_cut_timeout.load(Ordering::Relaxed), 0);
    assert_eq!(clean.stream_cut_transport.load(Ordering::Relaxed), 0);
}
