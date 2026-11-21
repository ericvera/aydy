//! The companion VS Code extension: what each window tells AYDY, and how AYDY
//! talks back to one particular window.
//!
//! Every VS Code window runs its own extension host process, and every Claude
//! session started in that window is a descendant of it. So a window's
//! extension host PID is the only identifier that names a window — VS Code
//! offers none — and reporting it is the whole of what registration is for.
//!
//! Both directions have to exist. The extension reports facts by posting to
//! AYDY's loopback listener; AYDY sends commands by posting to a loopback port
//! the extension binds and names at registration. A command has to reach one
//! window and no other: revealing a session in a window that does not host it
//! makes Claude Code open a second, live copy of that session.

pub mod command;
pub mod registry;
