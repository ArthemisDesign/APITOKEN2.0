use super::*;
use tokio::io::{duplex, AsyncWriteExt};

#[tokio::test]
async fn binary_frame_roundtrips_exact_non_utf8_bytes_under_fragmentation() {
    let (mut writer, mut reader) = duplex(1024);
    let payload = [0, 0xff, b'\n', b'{', 0x80, 1];
    tokio::spawn(async move {
        let mut frame = Vec::new();
        let mut header = [0u8; IPC_HEADER_BYTES];
        header[0] = IPC_KIND_DATA;
        header[1..9].copy_from_slice(&42u64.to_be_bytes());
        header[9..13].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        frame.extend_from_slice(&header);
        frame.extend_from_slice(&payload);
        for byte in frame {
            writer.write_all(&[byte]).await.unwrap();
        }
    });
    let (kind, id, actual) = read_raw_frame(&mut reader).await.unwrap().unwrap();
    assert_eq!(kind, IPC_KIND_DATA);
    assert_eq!(id, 42);
    assert_eq!(actual, payload);
}

#[tokio::test]
async fn oversized_control_is_rejected_from_header_before_payload_read() {
    let (mut writer, mut reader) = duplex(64);
    let mut header = [0u8; IPC_HEADER_BYTES];
    header[0] = IPC_KIND_CONTROL;
    header[9..13].copy_from_slice(&((MAX_IPC_CONTROL_BYTES as u32) + 1).to_be_bytes());
    writer.write_all(&header).await.unwrap();
    assert_eq!(
        read_raw_frame(&mut reader).await,
        Err(TransportError::Protocol)
    );
}

#[tokio::test]
async fn writer_emits_big_endian_header_and_raw_payload() {
    let (mut writer, mut reader) = duplex(128);
    let task = tokio::spawn(async move {
        write_raw_frame_locked(&mut writer, IPC_KIND_DATA, 7, b"raw\0bytes")
            .await
            .unwrap();
    });
    let (kind, id, payload) = read_raw_frame(&mut reader).await.unwrap().unwrap();
    task.await.unwrap();
    assert_eq!((kind, id), (IPC_KIND_DATA, 7));
    assert_eq!(payload, b"raw\0bytes");
}
