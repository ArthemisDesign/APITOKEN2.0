use super::transport::{
    TransportError, IPC_HEADER_BYTES, IPC_KIND_CONTROL, IPC_KIND_DATA, MAX_IPC_BODY_BYTES,
    MAX_IPC_CONTROL_BYTES, MAX_IPC_DATA_CHUNK_BYTES,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub(super) async fn write_raw_frame_locked<W: AsyncWrite + Unpin>(
    writer: &mut W,
    kind: u8,
    id: u64,
    payload: &[u8],
) -> Result<(), TransportError> {
    let length = u32::try_from(payload.len()).map_err(|_| TransportError::Protocol)?;
    let max = match kind {
        IPC_KIND_CONTROL => MAX_IPC_CONTROL_BYTES,
        IPC_KIND_DATA => MAX_IPC_BODY_BYTES,
        _ => return Err(TransportError::Protocol),
    };
    if payload.len() > max {
        return Err(TransportError::Protocol);
    }
    let mut header = [0u8; IPC_HEADER_BYTES];
    header[0] = kind;
    header[1..9].copy_from_slice(&id.to_be_bytes());
    header[9..13].copy_from_slice(&length.to_be_bytes());
    writer
        .write_all(&header)
        .await
        .map_err(|_| TransportError::Closed)?;
    writer
        .write_all(payload)
        .await
        .map_err(|_| TransportError::Closed)
}

pub(super) async fn read_raw_frame<R: AsyncRead + Unpin>(
    reader: &mut R,
) -> Result<Option<(u8, u64, Vec<u8>)>, TransportError> {
    let mut header = [0u8; IPC_HEADER_BYTES];
    match reader.read_exact(&mut header).await {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(_) => return Err(TransportError::Closed),
    }
    let kind = header[0];
    let id = u64::from_be_bytes(
        header[1..9]
            .try_into()
            .map_err(|_| TransportError::Protocol)?,
    );
    let length = u32::from_be_bytes(
        header[9..13]
            .try_into()
            .map_err(|_| TransportError::Protocol)?,
    ) as usize;
    let max = match kind {
        IPC_KIND_CONTROL => MAX_IPC_CONTROL_BYTES,
        IPC_KIND_DATA => MAX_IPC_DATA_CHUNK_BYTES,
        _ => return Err(TransportError::Protocol),
    };
    if length > max {
        return Err(TransportError::Protocol);
    }
    let mut payload = vec![0u8; length];
    reader
        .read_exact(&mut payload)
        .await
        .map_err(|_| TransportError::Closed)?;
    Ok(Some((kind, id, payload)))
}

#[cfg(test)]
mod tests;
