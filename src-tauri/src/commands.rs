use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use rand::Rng;
use tauri::State;

use crate::{
    database::Database,
    models::{AppSummary, ComparisonPair, PhotoSummary, RankingRow, ScanReport},
    pairing,
    photos::scan_directory,
};

pub struct AppState {
    inner: Arc<Mutex<InnerState>>,
    cache_directory: PathBuf,
}

struct InnerState {
    database: Database,
    current_pair: Option<(i64, i64)>,
}

impl AppState {
    pub fn new(database: Database, cache_directory: PathBuf) -> Self {
        Self {
            inner: Arc::new(Mutex::new(InnerState {
                database,
                current_pair: None,
            })),
            cache_directory,
        }
    }
}

#[tauri::command]
pub fn get_app_state(state: State<'_, AppState>) -> Result<AppSummary, String> {
    state.inner.lock().map_err(lock_error)?.database.summary()
}

#[tauri::command]
pub async fn set_photo_directory(
    path: String,
    state: State<'_, AppState>,
) -> Result<ScanReport, String> {
    scan_photo_directory(
        Arc::clone(&state.inner),
        state.cache_directory.join("thumbnails"),
        Some(PathBuf::from(path)),
    )
    .await
}

#[tauri::command]
pub async fn rescan_photo_directory(state: State<'_, AppState>) -> Result<ScanReport, String> {
    scan_photo_directory(
        Arc::clone(&state.inner),
        state.cache_directory.join("thumbnails"),
        None,
    )
    .await
}

#[tauri::command]
pub fn get_next_comparison(state: State<'_, AppState>) -> Result<Option<ComparisonPair>, String> {
    let mut inner = state.inner.lock().map_err(lock_error)?;
    let photos = inner.database.active_photos()?;
    let pair = pairing::next_pair(&photos, &inner.database.pair_counts()?, inner.current_pair)
        .map(randomize_pair);
    let Some(pair) = pair else {
        inner.current_pair = None;
        return Ok(None);
    };
    let left = photos
        .iter()
        .find(|photo| photo.id == pair.0)
        .ok_or_else(|| "无法找到左侧照片".to_string())?;
    let right = photos
        .iter()
        .find(|photo| photo.id == pair.1)
        .ok_or_else(|| "无法找到右侧照片".to_string())?;
    inner.current_pair = Some(pair);
    Ok(Some(ComparisonPair {
        left: PhotoSummary::from(left),
        right: PhotoSummary::from(right),
    }))
}

#[tauri::command]
pub fn record_comparison(winner_id: i64, state: State<'_, AppState>) -> Result<(), String> {
    let mut inner = state.inner.lock().map_err(lock_error)?;
    let pair = inner
        .current_pair
        .take()
        .ok_or_else(|| "当前没有待提交的比较".to_string())?;
    match inner.database.record_comparison(pair, winner_id) {
        Ok(()) => Ok(()),
        Err(error) => {
            inner.current_pair = Some(pair);
            Err(error)
        }
    }
}

#[tauri::command]
pub fn undo_last_comparison(state: State<'_, AppState>) -> Result<bool, String> {
    let mut inner = state.inner.lock().map_err(lock_error)?;
    let previous_pair = inner.current_pair.take();
    match inner.database.undo_last_comparison() {
        Ok(undone) => Ok(undone),
        Err(error) => {
            inner.current_pair = previous_pair;
            Err(error)
        }
    }
}

#[tauri::command]
pub fn get_rankings(state: State<'_, AppState>) -> Result<Vec<RankingRow>, String> {
    state.inner.lock().map_err(lock_error)?.database.rankings()
}

async fn scan_photo_directory(
    inner: Arc<Mutex<InnerState>>,
    thumbnail_directory: PathBuf,
    requested_directory: Option<PathBuf>,
) -> Result<ScanReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut inner = inner.lock().map_err(lock_error)?;
        let directory = match requested_directory {
            Some(directory) => directory,
            None => inner
                .database
                .summary()?
                .photo_directory
                .map(PathBuf::from)
                .ok_or_else(|| "请先选择照片目录".to_string())?,
        };
        let report = scan_directory(&directory, &mut inner.database, &thumbnail_directory)?;
        inner.current_pair = None;
        Ok(report)
    })
    .await
    .map_err(|error| format!("扫描任务失败：{error}"))?
}

fn randomize_pair(pair: (i64, i64)) -> (i64, i64) {
    if rand::thread_rng().gen_bool(0.5) {
        (pair.1, pair.0)
    } else {
        pair
    }
}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> String {
    "应用状态暂时不可用，请重试".to_string()
}
