//! Exercise the compiled capabilities through Tauri's real IPC authority.
//! No native WebView, URL getter or window system is needed for these checks.
use tauri::ipc::Origin;

const CONTROL: &[&str] = &[
    "launcher_start",
    "launcher_deploy",
    "launcher_stop",
    "launcher_repair",
    "launcher_check_update",
    "launcher_install_update",
    "launcher_open_webui",
    "launcher_cancel_and_exit",
    "launcher_cancel",
    "launcher_back",
    "settings_set",
];
const READ: &[&str] = &[
    "launcher_context",
    "launcher_snapshot",
    "launcher_show_home",
    "settings_get",
];
const BRIDGE: &[&str] = &[
    "save_as",
    "download_today_gui_log",
    "download_today_launcher_log",
    "download_log_archive",
    "prompt_log_archive_path",
    "window_hide",
    "window_minimize",
    "window_toggle_maximize",
    "window_close",
    "window_exit_application",
    "window_start_dragging",
    "window_is_maximized",
];

fn context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

#[test]
fn local_launcher_can_manage_service_updates_and_settings() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    for command in CONTROL.iter().chain(READ).chain(BRIDGE) {
        assert!(
            authority
                .resolve_access(command, "launcher", "launcher", &Origin::Local)
                .is_some(),
            "local launcher must allow {command}"
        );
    }
    for command in CONTROL {
        assert!(authority
            .resolve_access(command, "main", "main", &Origin::Local)
            .is_none());
        assert!(authority
            .resolve_access(command, "other", "other", &Origin::Local)
            .is_none());
    }
}

#[test]
fn local_connection_page_can_read_and_reconnect_but_cannot_manage_environment() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    for command in READ
        .iter()
        .chain(BRIDGE)
        .chain([&"retry_backend_connection"])
    {
        assert!(
            authority
                .resolve_access(command, "main", "main", &Origin::Local)
                .is_some(),
            "local connection page must allow {command}"
        );
    }
    assert!(authority
        .resolve_access(
            "retry_backend_connection",
            "launcher",
            "launcher",
            &Origin::Local
        )
        .is_none());
}

#[test]
fn backend_keeps_window_and_file_bridge_at_default_and_custom_ports() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    for port in [22267, 23456] {
        let origin = Origin::Remote {
            url: format!("http://127.0.0.1:{port}/?launcherTicket=fixture")
                .parse()
                .unwrap(),
        };
        for command in BRIDGE {
            assert!(
                authority
                    .resolve_access(command, "main", "main", &origin)
                    .is_some(),
                "backend must allow {command} at port {port}"
            );
        }
        for window in ["launcher", "main"] {
            for command in CONTROL
                .iter()
                .chain(READ)
                .chain([&"retry_backend_connection"])
            {
                assert!(
                    authority
                        .resolve_access(command, window, window, &origin)
                        .is_none(),
                    "backend must not allow {command} in {window}"
                );
            }
        }
    }
}

#[test]
fn unrelated_remote_origins_and_windows_have_no_command_access() {
    let mut context = context();
    let authority = context.runtime_authority_mut();
    for url in [
        "https://example.com/",
        "http://127.0.0.1.example.com:22267/",
        "http://192.0.2.1:22267/",
    ] {
        let origin = Origin::Remote {
            url: url.parse().unwrap(),
        };
        for window in ["launcher", "main", "other"] {
            for command in CONTROL
                .iter()
                .chain(READ)
                .chain(BRIDGE)
                .chain([&"retry_backend_connection"])
            {
                assert!(
                    authority
                        .resolve_access(command, window, window, &origin)
                        .is_none(),
                    "{url} must not allow {command} in {window}"
                );
            }
        }
    }
}
