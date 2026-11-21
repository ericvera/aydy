pub mod atomic_file;
pub mod clock;
pub mod discovery;
pub mod extension;
pub mod hooks;
pub mod ipc;
pub mod jump;
pub mod listener;
pub mod notify;
pub mod port_file;
pub mod session;
pub mod settings;
pub mod sync;
#[cfg(test)]
mod test_support;
pub mod tracking;
pub mod tray;

use std::sync::{Arc, Mutex};

use tauri::{Manager, RunEvent};
use tauri_plugin_autostart::MacosLauncher;

use crate::clock::SystemClock;
use crate::discovery::file_feed::{FeedShutdown, FileFeed};
use crate::discovery::process_tree::SystemParents;
use crate::discovery::reconciler::{user_sessions_dir, Reconciler, ScanShutdown};
use crate::discovery::resume::ResumeShutdown;
use crate::discovery::session_files::LiveProcesses;
use crate::discovery::status_files::DataDir;
use crate::discovery::transcript::user_projects_dir;
use crate::extension::command::{ExtensionCommands, LoopbackChannel};
use crate::extension::registry::ExtensionRegistry;
use crate::hooks::install::{follow_port_moves, HookInstaller, HookRegistry};
use crate::hooks::settings_file::SettingsFile;
use crate::hooks::stash::HookStash;
use crate::ipc::commands::{
    close_popover, extension_status, hook_status, install_hooks, jump_to_session, mark_read,
    preview_hook_install, quit, reinstall_hooks, reveal_settings_file, tracking_status,
    uninstall_hooks,
};
use crate::ipc::snapshot_channel::{subscribe_sessions, PublisherShutdown, SessionSubscription};
use crate::jump::{CodeCli, JumpRequests, Jumper, WindowRaiser};
use crate::listener::port::{PortMemory, PortState};
use crate::listener::server::{ListenerHealth, ListenerShutdown};
use crate::notify::sound::{SoundNotifier, SystemSoundPlayer};
use crate::port_file::{publish_port, PortFile, PORT_FILE};
use crate::session::registry::SessionRegistry;
use crate::settings::commands::{read_settings, update_settings, Autostart};
use crate::settings::store::{SettingsStore, SETTINGS_FILE};

/// The file the listener's port is remembered in, inside the app config
/// directory.
const PORT_MEMORY_FILE: &str = "listener-port";

/// The file the hook entries AYDY took over are stashed in, inside the app
/// config directory, so uninstall can put them back.
const REMOVED_HOOKS_FILE: &str = "removed-hooks.json";

/// The app version, as declared in `Cargo.toml` and baked in at compile time.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Starts the Tauri event loop.
///
/// AYDY lives in the menu bar, so on macOS the activation policy is set to
/// `Accessory`, which keeps the app out of the Dock and out of the app switcher.
/// The bundled `.app` gets the same treatment from `LSUIElement` in
/// `Info.plist`; this call is what covers `tauri dev`, where there is no bundle
/// and therefore no merged `Info.plist`.
///
/// The app is built rather than run directly so that the run event is
/// available: the hook listener has to be told to stop serving before the
/// process goes away.
pub fn run() {
    let app = tauri::Builder::default()
        // First, before any plugin that could bind or write something a second
        // copy would fight over. An instance launched at login and one launched
        // by hand would otherwise each bind a port and each rewrite the hook
        // URLs to their own, leaving the user's sessions reporting to whichever
        // wrote last.
        .plugin(tauri_plugin_single_instance::init(|app, _arguments, _cwd| {
            // Launching AYDY while it is already running is how someone checks
            // that it is: showing the popover answers that, and is the only
            // acknowledgement a Dock-less app can give.
            //
            // This is the path for a copy macOS did not recognise as the same
            // app — the binary run straight out of a build directory, or one
            // carrying a different bundle identifier. A launch of the installed
            // app arrives as `RunEvent::Reopen` instead, because macOS never
            // starts the second process at all.
            tray::popover::reveal(app);
        }))
        // `LaunchAgent` writes a plist into `~/Library/LaunchAgents/`. There is
        // no argument to pass: AYDY takes none, and an autostarted copy has to
        // behave exactly like a hand-launched one.
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        // The positioner has to be attached even though only Rust asks it to
        // move the window: the tray icon's position is kept in the plugin's own
        // managed state.
        .plugin(tauri_plugin_positioner::init())
        .invoke_handler(tauri::generate_handler![
            subscribe_sessions,
            mark_read,
            jump_to_session,
            close_popover,
            quit,
            hook_status,
            preview_hook_install,
            install_hooks,
            reinstall_hooks,
            uninstall_hooks,
            reveal_settings_file,
            extension_status,
            tracking_status,
            read_settings,
            update_settings
        ])
        .on_tray_icon_event(|app, event| tray::on_tray_event(app, &event))
        .on_window_event(tray::popover::on_window_event)
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let registry = Arc::new(Mutex::new(SessionRegistry::new(Arc::new(SystemClock))));
            let ports = Arc::new(PortState::new());
            let config = app.path().app_config_dir()?;
            let memory = PortMemory::new(config.join(PORT_MEMORY_FILE));
            let hooks = Arc::new(HookInstaller::new(
                SettingsFile::user()?,
                HookStash::new(config.join(REMOVED_HOOKS_FILE)),
                Arc::new(HookRegistry::new()),
            ));
            let health = Arc::new(ListenerHealth::new(Arc::clone(&ports), hooks.registry()));
            // One VS Code window, one registration, keyed by the extension
            // host PID it reports — the only identifier a window has.
            let windows = Arc::new(ExtensionRegistry::new(Arc::new(SystemClock)));
            let extensions = Arc::new(ExtensionCommands::new(
                Arc::clone(&windows),
                Arc::new(LoopbackChannel::new()),
            ));
            let published = PortFile::new(config.join(PORT_FILE));
            // The settings are loaded before anything that consumes them is
            // spawned, so nothing ever runs against the defaults for the first
            // moments of the app's life and then jumps.
            let settings = Arc::new(SettingsStore::load(
                config.join(SETTINGS_FILE),
                Arc::new(Autostart::new(app.handle().clone())),
            ));
            // The notifier holds a receiver on the settings rather than the
            // store, so a toggle moved in the pane is in force for the very
            // next transition without anything being re-read.
            //
            // It starts unarmed and is armed once the file feed has applied its
            // first pass, so replaying every status file at launch is the
            // silent baseline rather than a burst of sounds.
            let sounds = Arc::new(SoundNotifier::unarmed(
                Arc::new(SystemSoundPlayer),
                settings.subscribe(),
                Arc::new(SystemClock),
            ));
            let subscription = Arc::new(SessionSubscription::default());
            let (signal, shutdown) = tokio::sync::oneshot::channel();
            let (scan_signal, scan_shutdown) = tokio::sync::oneshot::channel();
            let (resume_signal, resume_shutdown) = tokio::sync::oneshot::channel();
            let (publish_signal, publish_shutdown) = tokio::sync::oneshot::channel();
            let (feed_signal, feed_shutdown) = tokio::sync::oneshot::channel();
            let data = DataDir::new(app.path().app_data_dir()?);
            let jumper = Arc::new(Jumper::new(
                Arc::new(CodeCli::default()) as Arc<dyn WindowRaiser>,
                JumpRequests::new(data.jump_dir()),
            ));

            app.manage(Arc::clone(&registry));
            app.manage(data.clone());
            app.manage(Arc::clone(&ports));
            app.manage(Arc::clone(&health));
            app.manage(Arc::clone(&hooks));
            app.manage(Arc::clone(&windows));
            app.manage(jumper);
            app.manage(published.clone());
            app.manage(settings);
            app.manage(Arc::clone(&subscription));
            app.manage(ListenerShutdown::new(signal));
            app.manage(ScanShutdown::new(scan_signal));
            app.manage(ResumeShutdown::new(resume_signal));
            app.manage(PublisherShutdown::new(publish_signal));
            app.manage(FeedShutdown::new(feed_signal));

            tauri::async_runtime::spawn(listener::server::run(
                memory,
                Arc::clone(&ports),
                Arc::clone(&registry),
                extensions,
                shutdown,
            ));
            tauri::async_runtime::spawn(follow_port_moves(hooks, Arc::clone(&ports)));
            // The companion extension has no setting to hold the port in, so
            // the port is published for it to read — and republished from the
            // same watch the hook URLs are rewritten from, so the two can never
            // name different ports.
            tauri::async_runtime::spawn(publish_port(published, Arc::clone(&ports)));

            tauri::async_runtime::spawn(discovery::file_feed::run(
                FileFeed::new(data, user_sessions_dir(), Arc::new(LiveProcesses)),
                Arc::clone(&registry),
                Box::new({
                    let sounds = Arc::clone(&sounds);
                    move || sounds.arm()
                }),
                feed_shutdown,
            ));

            // One loop feeds the menu bar icon, the popover and the sounds, so
            // all three are always derived from the same snapshot.
            tauri::async_runtime::spawn(ipc::snapshot_channel::run(
                app.handle().clone(),
                Arc::clone(&registry),
                health,
                Arc::clone(&ports),
                Arc::clone(&windows),
                subscription,
                sounds,
                publish_shutdown,
            ));

            // The resume watch runs on a cadence of its own, not on the scan's:
            // a session has to be back to Working within 5 seconds of resuming,
            // and the scan runs every 25.
            tauri::async_runtime::spawn(discovery::resume::run(
                Arc::clone(&registry),
                resume_shutdown,
            ));

            match (user_sessions_dir(), user_projects_dir()) {
                (Some(sessions), Some(projects)) => {
                    tauri::async_runtime::spawn(discovery::reconciler::run(
                        // The scan is where the session-to-window join runs:
                        // it already reads every session file, and reading them
                        // twice could place a session the rest of the pass
                        // never saw.
                        Reconciler::new(sessions, projects, Arc::new(LiveProcesses))
                            .with_windows(windows, Arc::new(SystemParents)),
                        registry,
                        scan_shutdown,
                    ));
                }
                _ => eprintln!(
                    "aydy: no home directory to look for session files in; tracking sessions from hook events only"
                ),
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to start the AYDY application");

    app.run(|handle, event| match event {
        // Asked to launch an app that is already running, macOS activates it
        // and sends this rather than starting a second process — so this, not
        // the single-instance callback, is the path a Spotlight or Finder
        // launch of the installed app takes. AYDY has no Dock icon and no
        // window for that activation to bring forward, so a reopen left
        // unhandled is a launch that does nothing whatsoever.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen { .. } => tray::popover::reveal(handle),
        RunEvent::ExitRequested { .. } | RunEvent::Exit => {
            // Withdrawn here rather than left to the publisher's watch: the
            // process may be gone before that task is scheduled again, and a
            // file left behind names a port AYDY has let go of.
            handle.state::<PortFile>().withdraw();
            handle.state::<ListenerShutdown>().trigger();
            handle.state::<ScanShutdown>().trigger();
            handle.state::<ResumeShutdown>().trigger();
            handle.state::<PublisherShutdown>().trigger();
            handle.state::<FeedShutdown>().trigger();
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_a_semver_triple() {
        let parts: Vec<&str> = version().split('.').collect();

        assert_eq!(
            parts.len(),
            3,
            "expected MAJOR.MINOR.PATCH, got {}",
            version()
        );
        assert!(
            parts.iter().all(|part| part.parse::<u32>().is_ok()),
            "expected numeric version components, got {}",
            version()
        );
    }
}
