use serde::Serialize;

pub const DEFAULT_MU: f64 = 25.0;
pub const DEFAULT_SIGMA: f64 = DEFAULT_MU / 3.0;

#[derive(Debug, Clone)]
pub struct PhotoRecord {
    pub id: i64,
    pub file_name: String,
    pub thumbnail_path: String,
    pub mu: f64,
    pub sigma: f64,
    pub comparison_count: u32,
    pub wins: u32,
    pub losses: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PhotoSummary {
    pub id: i64,
    pub file_name: String,
    pub thumbnail_path: String,
    pub mu: f64,
    pub sigma: f64,
}

impl From<&PhotoRecord> for PhotoSummary {
    fn from(photo: &PhotoRecord) -> Self {
        Self {
            id: photo.id,
            file_name: photo.file_name.clone(),
            thumbnail_path: photo.thumbnail_path.clone(),
            mu: photo.mu,
            sigma: photo.sigma,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparisonPair {
    pub left: PhotoSummary,
    pub right: PhotoSummary,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSummary {
    pub photo_directory: Option<String>,
    pub active_photo_count: u32,
    pub comparison_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub photo_directory: String,
    pub active_photo_count: u32,
    pub comparison_count: u32,
    pub added_count: u32,
    pub invalid_count: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankingRow {
    pub rank: u32,
    pub id: i64,
    pub file_name: String,
    pub thumbnail_path: String,
    pub ordinal: f64,
    pub mu: f64,
    pub sigma: f64,
    pub comparison_count: u32,
    pub wins: u32,
    pub losses: u32,
    pub win_rate: f64,
    pub sample_small: bool,
}
