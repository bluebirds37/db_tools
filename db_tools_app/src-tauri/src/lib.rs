pub mod ai;
mod commands;
pub mod db;
pub mod logger;
pub mod models;
pub mod store;
pub mod transfer;

use ai::tools::{SessionCache, SharedCache};
use store::AppState;
use std::sync::Arc;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState::new();
    logger::init(&state.data_dir);
    logger::install_panic_hook();
    logger::info("app", format!("starting db_tools, data_dir={:?}", state.data_dir));

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .manage(Arc::new(SessionCache::new()) as SharedCache)
        .invoke_handler(tauri::generate_handler![
            commands::list_connections,
            commands::save_connection,
            commands::delete_connection,
            commands::test_connection,
            commands::list_databases,
            commands::list_schemas,
            commands::list_tables,
            commands::list_columns,
            commands::list_indexes,
            commands::list_foreign_keys,
            commands::list_tablespaces,
            commands::describe_table_extra,
            commands::query_page,
            commands::execute_sql,
            commands::insert_row,
            commands::update_row,
            commands::delete_row,
            commands::reconnect,
            commands::list_ai_profiles,
            commands::save_ai_profile,
            commands::delete_ai_profile,
            commands::set_default_ai_profile,
            commands::list_chat_sessions,
            commands::create_chat_session,
            commands::delete_chat_session,
            commands::rename_chat_session,
            commands::list_memory_facts,
            commands::add_memory_fact,
            commands::clear_memory_facts,
            commands::ai_chat,
            commands::test_ai_connection,
            commands::log_frontend_error,
            commands::get_log_tail,
            commands::open_log_dir,
            commands::export_query,
            commands::dump_table,
            commands::dump_database,
            commands::sync_table,
            commands::sync_database,
            commands::write_text_file,
            commands::read_text_file,
            commands::import_tabular_file,
            commands::run_sql_script,
            commands::get_database_options,
            commands::plan_database,
            commands::apply_database,
            commands::list_extensions,
            commands::apply_extension,
            commands::plan_create_table,
            commands::apply_create_table,
            commands::migrate_database,
        ])
        .build(tauri::generate_context!())
        .map_err(|e| {
            logger::error("app", format!("build failed: {e}"));
            e
        })
        .expect("error while building tauri application")
        .run(|_app, event| {
            match event {
                tauri::RunEvent::Ready => {
                    logger::info("app", "ready");
                }
                tauri::RunEvent::Exit => logger::info("app", "exiting"),
                tauri::RunEvent::ExitRequested { .. } => {
                    logger::info("app", "exit requested");
                }
                tauri::RunEvent::WindowEvent { label, event, .. } => {
                    if let tauri::WindowEvent::Destroyed = event {
                        logger::info("window", format!("{label} destroyed"));
                    }
                }
                _ => {}
            }
        });
}
