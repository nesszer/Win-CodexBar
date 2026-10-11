//! Prometheus text exposition for bounded provider metrics.

const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

#[derive(Debug, PartialEq, Eq)]
pub(super) enum MetricsRenderError {
    DuplicateSeries(String),
}

mod definitions;
mod encoding;
mod rendering;
mod snapshot;

pub(crate) use snapshot::MetricsSnapshot;

pub(super) use rendering::metrics_response;
#[cfg(test)]
use rendering::render_at;

#[cfg(test)]
mod tests;
