mod commands;
mod database;
mod models;
mod pairing;
mod photos;
mod rating;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let cache_dir = app.path().app_cache_dir()?.join("face-rank");
            std::fs::create_dir_all(&data_dir)?;
            std::fs::create_dir_all(cache_dir.join("thumbnails"))?;

            let db = database::Database::open(data_dir.join("face-rank.sqlite"))?;
            app.asset_protocol_scope()
                .allow_directory(cache_dir.join("thumbnails"), true)?;
            app.manage(commands::AppState::new(db, cache_dir));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::set_photo_directory,
            commands::rescan_photo_directory,
            commands::get_next_comparison,
            commands::record_comparison,
            commands::undo_last_comparison,
            commands::get_rankings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
