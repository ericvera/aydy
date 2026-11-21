//! The loopback HTTP listener that receives Claude Code hook events.
//!
//! Claude Code's `http` hooks **block** the user's session until they answer,
//! so the endpoint answers immediately — always 200, always with an empty body
//! — and every event is applied to the registry off the request path. A body
//! that is neither empty nor a JSON object, and any status other than 200, both
//! surface as a hook error inside the user's Claude session, which is why the
//! handler never reports a payload it cannot understand.
//!
//! The port is chosen by the app, never by the user: the last one is
//! remembered so the registered hook URLs usually stay valid, and when it is
//! taken the OS assigns another and the move is signalled for the hook
//! installer to rewrite and for the popover to report.

pub mod port;
pub mod routes;
pub mod server;
