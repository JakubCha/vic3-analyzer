//! Tauri invoke handlers for the companion UI.
//!
//! Commands take stubs / config strings and return JSON DTOs. Absolute save
//! paths stay inside [`CompanionSession`](crate::session::CompanionSession).
//! Capability allowlist: `capabilities/default.json` + `allow-companion`.
//!
//! | Command | Role |
//! | --- | --- |
//! | `get_config` / `save_config` / `reset_config` | Settings ↔ [`vic3_catalog::AppConfig`] |
//! | `list_saves` / `get_dashboard` / `detection_hints` | Catalog + status |
//! | `use_save` | Stub → SQL bind + analysis session |
//! | `loaded_prices` / `loaded_summary` / `loaded_alerts` / `loaded_gaps` / `loaded_defs_icons` | Session analysis / defs icon JSON |
//! | `loaded_military` / `loaded_constructions` | Military + construction queue snapshots |
//! | `sql_query` / `sql_docs` | Advanced Query (shared shape with MCP) |
//! | `api_ping` | Smoke link to `vic3-api` |

use std::sync::Mutex;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::dto::{ConfigDto, DashboardDto, SaveStubDto};
use crate::session::{AppState, CompanionSession, SqlDocsDto};
use crate::watch::{self, WatchHandle};

/// Placeholder invoke proving `vic3-api` is linked into the desktop binary.
///
/// # Errors
///
/// Never fails today; `Result` kept for invoke shape consistency.
#[tauri::command]
pub fn api_ping() -> Result<&'static str, String> {
    let _ = vic3_api::ApiError::NoLoadedAnalysis;
    Ok("pong")
}

/// Return the current shared config DTO (paths as strings).
///
/// # Errors
///
/// `"state lock poisoned"` if the session mutex is poisoned.
#[tauri::command(async)]
pub fn get_config(state: State<'_, AppState>) -> Result<ConfigDto, String> {
    let session = state.inner.lock().map_err(|_| "state lock poisoned")?;
    Ok(session.config_dto())
}

/// Persist Settings and restart the save-dir watcher.
///
/// # Arguments
///
/// * `config` — DTO from the WebView; `config_path` on the client is ignored for writes.
///
/// # Errors
///
/// Lock poison, catalog/config I/O, or invalid paths from [`CompanionSession::apply_config`].
#[tauri::command(async)]
pub fn save_config(
    app: AppHandle,
    state: State<'_, AppState>,
    config: ConfigDto,
) -> Result<ConfigDto, String> {
    let dto = {
        let mut session = state.inner.lock().map_err(|_| "state lock poisoned")?;
        session.apply_config(config)?
    };
    restart_watch(&app, &state)?;
    Ok(dto)
}

/// Clear overrides, re-run auto-detect, persist, restart watch.
///
/// # Errors
///
/// Lock poison or config write / catalog refresh failure.
#[tauri::command(async)]
pub fn reset_config(app: AppHandle, state: State<'_, AppState>) -> Result<ConfigDto, String> {
    let dto = {
        let mut session = state.inner.lock().map_err(|_| "state lock poisoned")?;
        session.reset_to_auto_detect()?
    };
    restart_watch(&app, &state)?;
    Ok(dto)
}

/// Rescan allowlisted roots; return stub rows (no absolute paths).
///
/// # Errors
///
/// Lock poison or catalog I/O.
#[tauri::command(async)]
pub fn list_saves(state: State<'_, AppState>) -> Result<Vec<SaveStubDto>, String> {
    let mut session = state.inner.lock().map_err(|_| "state lock poisoned")?;
    session.refresh_catalog()
}

/// Dashboard status: detection flags, counts, hints, loaded stub.
///
/// # Errors
///
/// Lock poison.
#[tauri::command(async)]
pub fn get_dashboard(state: State<'_, AppState>) -> Result<DashboardDto, String> {
    let session = state.inner.lock().map_err(|_| "state lock poisoned")?;
    Ok(session.dashboard())
}

/// Pasteable path candidates when auto-detect fails.
#[tauri::command]
pub fn detection_hints() -> Vec<String> {
    crate::session::detection_hints()
}

/// Bind analysis + SQL session by filename stub.
///
/// Runs on a blocking thread pool so the WebView / UI thread stay responsive
/// (save parse + defs + price solve can take many seconds).
///
/// # Arguments
///
/// * `name` — stub (`autosave` or `autosave.v3`).
/// * `location` — optional `local` / `steam_cloud`.
/// * `solve_opts_json` — optional SolveOpts JSON (default `{}`).
///
/// # Errors
///
/// Ambiguous/missing stub, missing defs/tokens, load/solve failure (message string).
#[tauri::command]
pub async fn use_save(
    app: AppHandle,
    name: String,
    location: Option<String>,
    solve_opts_json: Option<String>,
) -> Result<String, String> {
    let opts = solve_opts_json.unwrap_or_else(|| "{}".into());
    let handle = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut session = state.inner.lock().map_err(|_| "state lock poisoned")?;
        session.use_save(&name, location.as_deref(), &opts)
    })
    .await
    .map_err(|e| format!("use_save join: {e}"))?;
    if result.is_ok() {
        // Other windows (the overlay) refresh their advice on this.
        let _ = handle.emit(ANALYSIS_LOADED_EVENT, ());
    }
    result
}

/// Newest in-game line from the Oikonomia Advisor mod (`logs/debug.log`), if any.
///
/// Returns `None` while a save is loading (the session lock is busy) so polling
/// never waits behind `use_save`.
#[tauri::command(async)]
pub fn live_status(state: State<'_, AppState>) -> Option<vic3_api::live::LiveStatus> {
    let dirs = state.inner.try_lock().ok()?.save_dirs().to_vec();
    let latest = vic3_api::live::latest(&dirs)?;
    // Keep the line: the game rotates its logs away after a few sessions.
    if let Some(store) = live_store() {
        let _ = vic3_api::live::merge_history(&store, std::slice::from_ref(&latest));
    }
    Some(latest)
}

/// Budget history kept across game sessions (`<app data>/live_history.jsonl`).
fn live_store() -> Option<std::path::PathBuf> {
    vic3_catalog::app_data_dir().ok().map(|dir| dir.join("live_history.jsonl"))
}

/// Charts over time for the loaded save: budget from the mod's log lines (all
/// sessions seen so far) and the histories stored in the save.
///
/// # Errors
///
/// No loaded analysis.
#[tauri::command(async)]
pub fn loaded_history(state: State<'_, AppState>) -> Result<String, String> {
    let dirs = state.inner.lock().map_err(|_| "state lock poisoned")?.save_dirs().to_vec();
    let lines = vic3_api::live::read_all_logs(&dirs);
    let lines = match live_store() {
        Some(store) => vic3_api::live::merge_history(&store, &lines).unwrap_or(lines),
        None => lines,
    };
    vic3_api::loaded_history_json(&lines).map_err(|e| e.to_string())
}

/// Event emitted to every window after `use_save` binds a new analysis.
pub const ANALYSIS_LOADED_EVENT: &str = "analysis-loaded";

/// Open the always-on-top "Now" overlay, or close it when open.
///
/// # Errors
///
/// Window creation failure (message string).
#[tauri::command]
pub async fn toggle_overlay(app: AppHandle) -> Result<bool, String> {
    if let Some(window) = app.get_webview_window("overlay") {
        window.close().map_err(|e| e.to_string())?;
        return Ok(false);
    }
    let mut builder = tauri::WebviewWindowBuilder::new(&app, "overlay", tauri::WebviewUrl::App("index.html?overlay".into()))
        .title("Vic3 Advisor · Now")
        .inner_size(380.0, 520.0)
        .always_on_top(true);
    if let Some(dir) = vic3_catalog::portable_data_dir() {
        builder = builder.data_directory(dir.join("webview"));
    }
    builder.build().map_err(|e| e.to_string())?;
    Ok(true)
}

/// Prices JSON for the bound analysis session (`vic3-api`).
///
/// # Errors
///
/// No loaded analysis.
///
/// The `loaded_*` reads below use the process-wide `vic3-api` session without
/// the `AppState` lock, so they never wait behind a running `use_save`.
#[tauri::command(async)]
pub fn loaded_prices() -> Result<String, String> {
    vic3_api::loaded_prices_json().map_err(|e| e.to_string())
}

/// Defs icons JSON (PNG data URLs) from the resolved defs postcard.
///
/// Off the UI thread — base64 icon encoding can be heavy for full game defs.
///
/// # Errors
///
/// Missing/invalid defs, I/O, encode failure, or lock poison.
#[tauri::command]
pub async fn loaded_defs_icons(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        // Snapshot config, then release the lock: a first defs build can take minutes.
        let (config, app_data) = {
            let state = app.state::<AppState>();
            let session = state.inner.lock().map_err(|_| "state lock poisoned")?;
            session.defs_context()
        };
        crate::session::defs_icons_json(&config, &app_data)
    })
    .await
    .map_err(|e| format!("loaded_defs_icons join: {e}"))?
}

/// Save summary JSON (tag, market/country ids, building type ids) for the bound session.
///
/// # Errors
///
/// No loaded analysis.
#[tauri::command(async)]
pub fn loaded_summary() -> Result<String, String> {
    vic3_api::loaded_summary_json().map_err(|e| e.to_string())
}

/// Military snapshot JSON for the bound session.
///
/// # Errors
///
/// No loaded analysis.
#[tauri::command(async)]
pub fn loaded_military() -> Result<String, String> {
    vic3_api::loaded_military_json().map_err(|e| e.to_string())
}

/// Construction queue snapshot JSON for the bound session.
///
/// # Errors
///
/// No loaded analysis.
#[tauri::command(async)]
pub fn loaded_constructions() -> Result<String, String> {
    vic3_api::loaded_constructions_json().map_err(|e| e.to_string())
}

/// Alerts JSON for the bound analysis session.
///
/// # Errors
///
/// No loaded analysis.
#[tauri::command(async)]
pub fn loaded_alerts() -> Result<String, String> {
    vic3_api::loaded_alerts_json().map_err(|e| e.to_string())
}

/// Advisor JSON (economy / laws / war) for the loaded analysis.
///
/// Reads the process-wide `vic3-api` session without the `AppState` lock and
/// off the UI thread, so it never waits behind a running `use_save`.
///
/// # Errors
///
/// No loaded analysis.
#[tauri::command(async)]
pub fn loaded_advice() -> Result<String, String> {
    vic3_api::loaded_advice_json().map_err(|e| e.to_string())
}

/// Evaluate tracked advice checks on the loaded save.
///
/// # Arguments
///
/// * `checks` — JSON array of `vic3_api::advice::Check`.
///
/// # Errors
///
/// No loaded analysis, or invalid JSON.
#[tauri::command(async)]
pub fn loaded_checks(checks: String) -> Result<String, String> {
    vic3_api::loaded_checks_json(&checks).map_err(|e| e.to_string())
}

/// Gaps JSON for `goal` against the bound session.
///
/// # Arguments
///
/// * `goal` — DSL string (see `docs/dsl.md`).
///
/// # Errors
///
/// No loaded analysis, or goal parse/plan failure.
#[tauri::command]
pub async fn loaded_gaps(goal: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        vic3_api::loaded_gaps_json(&goal).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("loaded_gaps join: {e}"))?
}

/// Advanced Query: one read-only statement → `{ columns, rows, row_count }` JSON.
///
/// Same result shape as MCP `query` (`docs/mcp.md` / `docs/sql.md`).
/// Off the UI thread so long queries do not beachball the WebView.
///
/// # Errors
///
/// SQL / session errors as strings; lock poison.
#[tauri::command]
pub async fn sql_query(app: AppHandle, sql: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let mut session = state.inner.lock().map_err(|_| "state lock poisoned")?;
        session.sql_query(&sql)
    })
    .await
    .map_err(|e| format!("sql_query join: {e}"))?
}

/// Advanced Query docs panel: `docs/sql.md` + UDF index (future `vic3://docs/sql`).
///
/// # Errors
///
/// Lock poison.
#[tauri::command(async)]
pub fn sql_docs(state: State<'_, AppState>) -> Result<SqlDocsDto, String> {
    let session = state.inner.lock().map_err(|_| "state lock poisoned")?;
    Ok(session.sql_docs())
}

/// Query the current detection and configuration status of supported MCP clients on this OS.
#[tauri::command(async)]
pub fn get_mcp_status() -> Vec<vic3_catalog::McpClientStatus> {
    vic3_catalog::McpClientKind::supported_on_current_os()
        .into_iter()
        .map(|c| c.status())
        .collect()
}

/// Enable or disable MCP configuration for a specific AI client.
#[tauri::command(async)]
pub fn toggle_mcp_client(
    client_id: String,
    enabled: bool,
) -> Result<vic3_catalog::McpClientStatus, String> {
    let client = vic3_catalog::McpClientKind::from_id_or_alias(&client_id)
        .ok_or_else(|| format!("unknown client '{client_id}'"))?;
    if enabled {
        let binary = vic3_catalog::resolve_mcp_binary(None);
        vic3_catalog::install_client_config(client, &binary, false)?;
    } else {
        vic3_catalog::uninstall_client_config(client, false)?;
    }
    Ok(client.status())
}

fn restart_watch(app: &AppHandle, state: &State<'_, AppState>) -> Result<(), String> {
    let roots = {
        let session = state.inner.lock().map_err(|_| "state lock poisoned")?;
        session.save_dirs().to_vec()
    };
    if let Some(slot) = app.try_state::<Mutex<Option<WatchHandle>>>() {
        let mut guard = slot.lock().map_err(|_| "watch lock poisoned")?;
        watch::restart_watcher(app, &mut guard, roots);
    }
    Ok(())
}

/// Register managed state and start the initial save-dir watcher.
///
/// # Errors
///
/// Propagates Tauri setup errors. Config load failure falls back to a temp
/// app-data dir (logged) rather than failing startup.
pub fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let session = CompanionSession::open(None).unwrap_or_else(|err| {
        eprintln!("vic3-analyzer: config load failed ({err}); using empty defaults");
        let fallback = tempfile_app_data();
        CompanionSession::open(Some(fallback)).expect("fallback session")
    });
    let roots = session.save_dirs().to_vec();
    app.manage(AppState::new(session));
    let handle = watch::spawn_save_watcher(app.handle().clone(), roots);
    app.manage(Mutex::new(handle));

    // The window is built here (`create: false` in tauri.conf.json) so a portable
    // copy keeps WebView2 data in `data/webview` beside the exe, not the profile.
    let config = app.config().app.windows[0].clone();
    let mut window = tauri::WebviewWindowBuilder::from_config(app.handle(), &config)?;
    if let Some(dir) = vic3_catalog::portable_data_dir() {
        window = window.data_directory(dir.join("webview"));
    }
    window.build()?;
    Ok(())
}

fn tempfile_app_data() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("vic3-analyzer-fallback-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}
