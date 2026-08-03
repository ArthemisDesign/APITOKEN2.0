//! Shared bounded readers for loopback authority responses.
//!
//! Provider runtimes are trusted producers, but a mixed-version or faulty slot must not make the
//! router allocate an unbounded response. Callers choose a contract-specific byte ceiling and can
//! distinguish an oversized response from a transport/body failure for bounded observability.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReadError {
    Oversized,
    Transport,
}

pub async fn response_bytes(
    mut response: reqwest::Response,
    limit: usize,
) -> Result<Vec<u8>, ReadError> {
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(ReadError::Oversized);
    }

    let mut body = Vec::with_capacity(
        response
            .content_length()
            .and_then(|length| usize::try_from(length).ok())
            .unwrap_or(0)
            .min(limit),
    );
    loop {
        match response.chunk().await {
            Ok(Some(chunk)) => {
                if body.len().saturating_add(chunk.len()) > limit {
                    return Err(ReadError::Oversized);
                }
                body.extend_from_slice(&chunk);
            }
            Ok(None) => return Ok(body),
            Err(_) => return Err(ReadError::Transport),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::response::Response;
    use axum::routing::get;
    use axum::Router;

    async fn origin(body: &'static str) -> String {
        let app = Router::new().route(
            "/",
            get(move || async move { Response::new(Body::from(body)) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        format!("http://{address}")
    }

    #[tokio::test]
    async fn rejects_oversized_response() {
        let response = reqwest::get(origin("12345").await).await.unwrap();
        assert_eq!(response_bytes(response, 4).await, Err(ReadError::Oversized));
    }
}
