//! The session model: identity, status, timing, unread state, grouping and
//! ordering.
//!
//! Nothing here performs I/O. The listener, the reconciliation scan and
//! transcript enrichment all feed this module, and every timing decision reads
//! the injected clock, so the whole status model is exercised by unit tests.

pub mod event;
pub mod registry;
#[allow(clippy::module_inception)]
pub mod session;
pub mod status;
pub mod tab_match;
pub mod timing;
