//! Mock-server helpers shared by provider tests.

use mockito::{Matcher, Mock, ServerGuard};

/// A mock answering `method path` with `status` and `body`.
pub(crate) async fn mock_response(
    server: &mut ServerGuard,
    method: &str,
    path: impl Into<Matcher>,
    status: usize,
    body: impl AsRef<[u8]>,
) -> Mock {
    server
        .mock(method, path)
        .with_status(status)
        .with_body(body)
        .create_async()
        .await
}

/// [`mock_response`] that expects exactly `hits` requests.
pub(crate) async fn mock_response_expect(
    server: &mut ServerGuard,
    method: &str,
    path: impl Into<Matcher>,
    status: usize,
    body: impl AsRef<[u8]>,
    hits: usize,
) -> Mock {
    server
        .mock(method, path)
        .with_status(status)
        .with_body(body)
        .expect(hits)
        .create_async()
        .await
}

/// A mock answering `method path` with `status` and an empty body.
pub(crate) async fn mock_status(
    server: &mut ServerGuard,
    method: &str,
    path: impl Into<Matcher>,
    status: usize,
) -> Mock {
    server
        .mock(method, path)
        .with_status(status)
        .create_async()
        .await
}

/// [`mock_status`] that expects exactly `hits` requests.
pub(crate) async fn mock_status_expect(
    server: &mut ServerGuard,
    method: &str,
    path: impl Into<Matcher>,
    status: usize,
    hits: usize,
) -> Mock {
    server
        .mock(method, path)
        .with_status(status)
        .expect(hits)
        .create_async()
        .await
}
