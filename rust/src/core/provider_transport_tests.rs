use super::ProviderError;

#[test]
fn typed_transport_classification_does_not_trust_free_form_messages() {
    assert!(ProviderError::Timeout.is_transport_failure());
    for error in [
        ProviderError::AuthRequired,
        ProviderError::Parse("invalid response".to_string()),
        ProviderError::Other("Network error: localized failure".to_string()),
        ProviderError::Other("Transport error: connection lost".to_string()),
    ] {
        assert!(!error.is_transport_failure());
    }
}

#[tokio::test]
async fn malformed_request_is_not_a_retainable_transport_failure() {
    let error = reqwest::Client::new()
        .get("not a URL")
        .send()
        .await
        .expect_err("invalid URL should fail before a network request");
    assert!(!ProviderError::Network(error).is_transport_failure());
}

#[tokio::test]
async fn refused_connection_is_a_retainable_transport_failure() {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))
        .expect("a local ephemeral port should be available");
    let address = listener
        .local_addr()
        .expect("the local listener should expose its address");
    drop(listener);

    let error = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("the test client should build")
        .get(format!("http://{address}/"))
        .send()
        .await
        .expect_err("the closed local port should refuse the connection");

    assert!(error.is_connect(), "expected a connect error: {error:?}");
    assert!(ProviderError::Network(error).is_transport_failure());
}

#[tokio::test]
async fn tls_handshake_failure_is_not_a_retainable_transport_failure() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local ephemeral port should be available");
    let address = listener
        .local_addr()
        .expect("the local listener should expose its address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("the client should connect to the local listener");
        let mut client_hello_byte = [0_u8; 1];
        stream
            .read_exact(&mut client_hello_byte)
            .await
            .expect("the TLS client hello should be readable");
        stream
            .write_all(b"HTTP/1.1 400 Bad Request\r\nConnection: close\r\n\r\n")
            .await
            .expect("the local TLS peer should respond");
    });

    let error = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(1))
        .build()
        .expect("the test client should build")
        .get(format!("https://{address}/"))
        .send()
        .await
        .expect_err("a plaintext peer must fail the TLS handshake");

    server.await.expect("the local TLS peer should finish");
    assert!(error.is_connect(), "expected a connect error: {error:?}");
    assert!(!ProviderError::Network(error).is_transport_failure());
}

#[tokio::test]
async fn truncated_response_body_is_not_a_retainable_transport_failure() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local ephemeral port should be available");
    let address = listener
        .local_addr()
        .expect("the local listener should expose its address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("the client should connect to the local listener");
        let mut request = [0_u8; 1024];
        stream
            .read(&mut request)
            .await
            .map(|_| ())
            .expect("the local request should be readable");
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 32\r\nConnection: close\r\n\r\npartial")
            .await
            .expect("the local response should be writable");
    });

    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("the test client should build")
        .get(format!("http://{address}/"))
        .send()
        .await
        .expect("the response headers should be valid");
    let error = response
        .bytes()
        .await
        .expect_err("the truncated response body should fail");

    server.await.expect("the local response task should finish");
    assert!(error.is_decode(), "expected a body decode error: {error:?}");
    assert!(!ProviderError::Network(error).is_transport_failure());
}

#[tokio::test]
async fn response_body_timeout_is_not_a_retainable_transport_failure() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local ephemeral port should be available");
    let address = listener
        .local_addr()
        .expect("the local listener should expose its address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("the client should connect to the local listener");
        let mut request = [0_u8; 1024];
        stream
            .read(&mut request)
            .await
            .map(|_| ())
            .expect("the local request should be readable");
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 32\r\nConnection: keep-alive\r\n\r\npartial",
            )
            .await
            .expect("the local response should be writable");
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    });

    let response = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_millis(50))
        .build()
        .expect("the test client should build")
        .get(format!("http://{address}/"))
        .send()
        .await
        .expect("the response headers should be valid");
    let error = response
        .bytes()
        .await
        .expect_err("the stalled response body should time out");

    server.abort();
    assert!(
        server.await.is_err(),
        "the stalled server should be cancelled"
    );
    assert!(error.is_decode(), "expected a body decode error: {error:?}");
    assert!(error.is_timeout(), "expected a body timeout: {error:?}");
    assert!(!ProviderError::Network(error).is_transport_failure());
}

#[tokio::test]
async fn request_body_failures_are_not_retainable_transport_failures() {
    use futures::stream;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("a local ephemeral port should be available");
    let address = listener
        .local_addr()
        .expect("the local listener should expose its address");
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener
            .accept()
            .await
            .expect("the client should connect to the local listener");
        let mut request = [0_u8; 1024];
        stream
            .read(&mut request)
            .await
            .map(|_| ())
            .expect("the local request should be readable");
    });
    let body = reqwest::Body::wrap_stream(stream::once(async {
        Err::<Vec<u8>, _>(std::io::Error::other("synthetic request body failure"))
    }));

    let error = reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("the test client should build")
        .post(format!("http://{address}/"))
        .body(body)
        .send()
        .await
        .expect_err("the request body should fail");

    server.await.expect("the local request task should finish");
    assert!(error.is_request(), "expected a request error: {error:?}");
    assert!(!ProviderError::Network(error).is_transport_failure());
}
