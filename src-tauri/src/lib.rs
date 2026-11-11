//! GSpot: a local-first Grammarly-style writing assistant for macOS and Windows.
//! Native Tauri host + Harper engine + Accessibility / UI Automation overlays + LLM features.

pub mod aiphrases;
pub mod apply;
pub mod ax;
pub mod commands;
pub mod debug_server;
pub mod engine;
pub mod features;
pub mod geom;
pub mod history;
pub mod host;
pub mod llm;
pub mod overlay;
pub mod platform;
pub mod publik;
pub mod publik_gate;
pub mod score;
pub mod settings;
pub mod textmap;
pub mod uia;

use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};

pub fn run() {
    // Builder check (publik setup skill): PUBLIK_SELF_TEST=1 prints one line and exits.
    if std::env::var_os("PUBLIK_SELF_TEST").is_some() {
        println!("{}", publik::self_test());
        return;
    }
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).try_init();

    let mut state = host::HostState::default();
    state.settings = settings::load();
    state.history = history::load();
    let shared: host::Shared = Arc::new(Mutex::new(state));
    let dock_side = shared.lock().unwrap().settings.dock_side.clone();

    tauri::Builder::default()
        .manage(shared.clone())
        .register_uri_scheme_protocol(publik_gate::SCHEME, publik_gate::protocol)
        .setup(move |app| {
            // Sign in with publik comes first: the gate window is the only window until the
            // person is signed in. Then start_app builds the overlay windows and the tray.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let (shared, dock_side) = (shared.clone(), dock_side.clone());
            publik_gate::run(app.handle().clone(), move |handle, (_key, wallet, choice)| {
                publik::remember_wallet(&wallet);
                if let Err(e) = start_app(handle, &shared, &dock_side, choice.as_deref()) {
                    log::error!("GSpot could not start: {e}");
                    handle.exit(1);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "panel" {
                    api.prevent_close();
                    commands::tuck_panel_inner(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::accept_alert,
            commands::dismiss_alert,
            commands::replace_range,
            commands::run_feature,
            commands::chat_send,
            commands::get_settings,
            commands::set_settings,
            commands::get_history,
            commands::open_panel,
            commands::hide_panel,
            commands::tuck_panel,
            commands::permission_prompt,
            commands::permission_status,
            commands::set_card_rects,
            commands::open_url,
            commands::quit_app,
            commands::ui_ready,
            commands::ui_log,
            commands::publik_status,
            commands::publik_accept_disclosure,
            commands::publik_dismiss_first_run,
            commands::publik_forget,
            commands::publik_refresh,
            commands::probe_key,
        ])
        .run(tauri::generate_context!())
        .expect("error while running GSpot");
}

/// The app proper, once the gate let the person through: the gate's answer goes to
/// Settings → AI, then the overlay windows, the tray and the host threads start.
fn start_app(app: &AppHandle, shared: &host::Shared, dock_side: &str, choice: Option<&str>) -> tauri::Result<()> {
    {
        let mut st = shared.lock().unwrap();
        let before = st.settings.llm.provider.clone();
        publik::apply_choice(&mut st.settings, choice);
        if st.settings.llm.provider != before {
            let _ = settings::save(&st.settings);
        }
    }
    // A menu-bar app again: the gate window was the one window in the Dock.
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    overlay::create_windows(app, dock_side)?;
    // Start tucked behind the edge tab, like Grammarly. First run (no
    // Accessibility yet) starts open so the permission gate is visible.
    overlay::set_tucked(app, dock_side, platform::is_trusted(), false, false);

    let open = MenuItem::with_id(app, "open", "Open GSpot", true, None::<&str>)?;
    let perms = MenuItem::with_id(app, "perms", "Accessibility permission…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit GSpot", true, None::<&str>)?;
    // Windows needs no screen-reading permission, so its tray menu has no such item.
    let menu = if platform::NEEDS_PERMISSION { Menu::with_items(app, &[&open, &perms, &quit])? } else { Menu::with_items(app, &[&open, &quit])? };
    let mut tray = TrayIconBuilder::with_id("main").menu(&menu).show_menu_on_left_click(true).tooltip("GSpot");
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.on_menu_event(|app, event| match event.id.as_ref() {
        "open" => commands::open_panel_inner(app, "review", true),
        "perms" => {
            platform::prompt_trust();
        }
        "quit" => app.exit(0),
        _ => {}
    })
    .build(app)?;

    host::spawn(app.clone(), shared.clone());
    debug_server::spawn(app.clone(), shared.clone());
    if !platform::is_trusted() {
        platform::prompt_trust();
    }
    Ok(())
}
