use super::*;
use futures_util::StreamExt;

fn body_of(chunks: Vec<Result<bytes::Bytes, std::io::Error>>) -> ResponseByteStream {
    Box::pin(futures_util::stream::iter(chunks))
}

#[tokio::test]
async fn a_broken_stream_ends_with_the_protocol_error_frame() {
    // Without this the body just stops. An SDK cannot tell a truncated stream from a finished
    // one until something further up fails to parse, and many clients wait forever instead.
    let inner = body_of(vec![
        Ok(bytes::Bytes::from_static(b"event: message_start\n\n")),
        Err(std::io::Error::other("upstream vanished")),
    ]);
    let collected: Vec<_> = SseErrorTail::new(inner).collect().await;
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
    let collected: Vec<_> = SseErrorTail::new(inner).collect().await;
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
    let collected: Vec<_> = SseErrorTail::new(inner).collect().await;
    assert_eq!(collected.len(), 1);
    assert!(String::from_utf8(collected[0].as_ref().unwrap().to_vec())
        .unwrap()
        .starts_with("event: error\n"));
}
