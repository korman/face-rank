use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::{
    models::{AppSummary, PhotoRecord, RankingRow, DEFAULT_MU, DEFAULT_SIGMA},
    rating::{self, MODEL_VERSION},
};

const SCHEMA_VERSION: u32 = 1;
type PairCount = ((i64, i64), u32);

pub struct Database {
    connection: Connection,
}

pub(crate) struct PhotoScan<'connection> {
    transaction: Transaction<'connection>,
}

impl PhotoScan<'_> {
    pub fn set_photo_directory(&self, directory: &str) -> Result<(), String> {
        set_photo_directory_on(&self.transaction, directory)
    }

    pub fn upsert_photo(
        &self,
        path: &str,
        file_name: &str,
        source_size: i64,
        source_modified_ns: i64,
    ) -> Result<(i64, bool, bool), String> {
        upsert_photo_on(
            &self.transaction,
            path,
            file_name,
            source_size,
            source_modified_ns,
        )
    }

    pub fn update_thumbnail_path(&self, id: i64, thumbnail_path: &str) -> Result<(), String> {
        update_thumbnail_path_on(&self.transaction, id, thumbnail_path)
    }

    pub fn active_count(&self) -> Result<u32, String> {
        active_count_on(&self.transaction)
    }

    pub fn comparison_count(&self) -> Result<u32, String> {
        comparison_count_on(&self.transaction)
    }

    pub fn thumbnail_paths(&self) -> Result<Vec<String>, String> {
        let mut statement = self
            .transaction
            .prepare("SELECT thumbnail_path FROM photos WHERE active = 1 AND thumbnail_path <> ''")
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| row.get(0))
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub fn commit(self) -> Result<(), String> {
        self.transaction.commit().map_err(db_error)
    }
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(db_error)?;
        Self::from_connection(connection)
    }

    #[cfg(test)]
    pub fn in_memory() -> Result<Self, String> {
        let connection = Connection::open_in_memory().map_err(db_error)?;
        Self::from_connection(connection)
    }

    fn from_connection(mut connection: Connection) -> Result<Self, String> {
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(db_error)?;
        let schema_version: u32 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(db_error)?;
        if schema_version > SCHEMA_VERSION {
            return Err(format!(
                "数据库版本 {schema_version} 高于当前程序支持的版本 {SCHEMA_VERSION}"
            ));
        }
        if schema_version < 1 {
            let transaction = connection.transaction().map_err(db_error)?;
            transaction
                .execute_batch(
                    "CREATE TABLE IF NOT EXISTS settings (
                   key TEXT PRIMARY KEY,
                   value TEXT NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS photos (
                   id INTEGER PRIMARY KEY AUTOINCREMENT,
                   path TEXT NOT NULL UNIQUE,
                   file_name TEXT NOT NULL,
                   thumbnail_path TEXT NOT NULL,
                   source_size INTEGER NOT NULL DEFAULT 0,
                   source_modified_ns INTEGER NOT NULL DEFAULT 0,
                   mu REAL NOT NULL DEFAULT 25.0,
                   sigma REAL NOT NULL DEFAULT 8.333333333333334,
                   comparison_count INTEGER NOT NULL DEFAULT 0,
                   wins INTEGER NOT NULL DEFAULT 0,
                   losses INTEGER NOT NULL DEFAULT 0,
                   active INTEGER NOT NULL DEFAULT 1 CHECK (active IN (0, 1)),
                   created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                   updated_at INTEGER NOT NULL DEFAULT (unixepoch())
                 );
                 CREATE TABLE IF NOT EXISTS comparisons (
                   id INTEGER PRIMARY KEY AUTOINCREMENT,
                   left_photo_id INTEGER NOT NULL REFERENCES photos(id),
                   right_photo_id INTEGER NOT NULL REFERENCES photos(id),
                   winner_photo_id INTEGER NOT NULL REFERENCES photos(id),
                   model_version TEXT NOT NULL,
                   created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                   CHECK (left_photo_id <> right_photo_id),
                   CHECK (winner_photo_id = left_photo_id OR winner_photo_id = right_photo_id)
                 );
                 CREATE INDEX IF NOT EXISTS idx_photos_active ON photos(active);
                 CREATE INDEX IF NOT EXISTS idx_comparisons_pair
                   ON comparisons(left_photo_id, right_photo_id);
                 PRAGMA user_version = 1;",
                )
                .map_err(db_error)?;
            transaction.commit().map_err(db_error)?;
        }

        let database = Self { connection };
        database.ensure_rating_model_compatible()?;
        Ok(database)
    }

    fn ensure_rating_model_compatible(&self) -> Result<(), String> {
        if let Some(configured_version) = self.setting("rating_model_version")? {
            if configured_version != MODEL_VERSION {
                return Err(format!(
                    "评分模型不兼容：数据库使用 {configured_version}，当前程序使用 {MODEL_VERSION}"
                ));
            }
        }
        let event_version = self
            .connection
            .query_row(
                "SELECT model_version FROM comparisons WHERE model_version <> ?1 LIMIT 1",
                [MODEL_VERSION],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(db_error)?;
        if let Some(event_version) = event_version {
            return Err(format!("比较记录使用了不兼容的评分模型：{event_version}"));
        }
        self.connection
            .execute(
                "INSERT OR IGNORE INTO settings(key, value) VALUES('rating_model_version', ?1)",
                [MODEL_VERSION],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn begin_photo_scan(&mut self) -> Result<PhotoScan<'_>, String> {
        let transaction = self.connection.transaction().map_err(db_error)?;
        begin_scan_on(&transaction)?;
        Ok(PhotoScan { transaction })
    }

    pub fn summary(&self) -> Result<AppSummary, String> {
        let photo_directory = self.setting("photo_directory")?;
        let active_photo_count = self
            .connection
            .query_row("SELECT COUNT(*) FROM photos WHERE active = 1", [], |row| {
                row.get(0)
            })
            .map_err(db_error)?;
        let comparison_count = self.comparison_count()?;
        Ok(AppSummary {
            photo_directory,
            active_photo_count,
            comparison_count,
        })
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>, String> {
        self.connection
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()
            .map_err(db_error)
    }

    #[cfg(test)]
    pub fn begin_scan(&self) -> Result<(), String> {
        begin_scan_on(&self.connection)
    }

    pub fn comparison_count(&self) -> Result<u32, String> {
        comparison_count_on(&self.connection)
    }

    #[cfg(test)]
    pub fn set_photo_active(&self, id: i64, active: bool) -> Result<(), String> {
        set_photo_active_on(&self.connection, id, active)
    }

    #[cfg(test)]
    pub fn upsert_photo(
        &self,
        path: &str,
        file_name: &str,
        source_size: i64,
        source_modified_ns: i64,
    ) -> Result<(i64, bool, bool), String> {
        upsert_photo_on(
            &self.connection,
            path,
            file_name,
            source_size,
            source_modified_ns,
        )
    }

    #[cfg(test)]
    pub fn reject_photo_inserts_for_test(&self) -> Result<(), String> {
        self.connection
            .execute_batch(
                "CREATE TRIGGER reject_photo_insert
                 BEFORE INSERT ON photos
                 BEGIN
                   SELECT RAISE(FAIL, 'simulated photo insert failure');
                 END;",
            )
            .map_err(db_error)
    }

    pub fn active_photos(&self) -> Result<Vec<PhotoRecord>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT id, file_name, thumbnail_path, mu, sigma,
                        comparison_count, wins, losses
                 FROM photos WHERE active = 1 ORDER BY id",
            )
            .map_err(db_error)?;
        let rows = statement.query_map([], photo_from_row).map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub fn pair_counts(&self) -> Result<Vec<PairCount>, String> {
        let mut statement = self
            .connection
            .prepare(
                "SELECT MIN(left_photo_id, right_photo_id), MAX(left_photo_id, right_photo_id), COUNT(*)
                 FROM comparisons GROUP BY 1, 2",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| Ok(((row.get(0)?, row.get(1)?), row.get(2)?)))
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
    }

    pub fn record_comparison(&mut self, pair: (i64, i64), winner_id: i64) -> Result<(), String> {
        if pair.0 == pair.1 || (winner_id != pair.0 && winner_id != pair.1) {
            return Err("胜者不属于当前比较组合".to_string());
        }
        let loser_id = if winner_id == pair.0 { pair.1 } else { pair.0 };
        let transaction = self.connection.transaction().map_err(db_error)?;
        let active_photo_count: u32 = transaction
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE id IN (?1, ?2) AND active = 1",
                params![pair.0, pair.1],
                |row| row.get(0),
            )
            .map_err(db_error)?;
        if active_photo_count != 2 {
            return Err("当前比较包含未启用的照片".to_string());
        }
        apply_result(&transaction, winner_id, loser_id)?;
        transaction
            .execute(
                "INSERT INTO comparisons(left_photo_id, right_photo_id, winner_photo_id, model_version)
                 VALUES(?1, ?2, ?3, ?4)",
                params![pair.0, pair.1, winner_id, MODEL_VERSION],
            )
            .map_err(db_error)?;
        transaction.commit().map_err(db_error)
    }

    pub fn undo_last_comparison(&mut self) -> Result<bool, String> {
        let transaction = self.connection.transaction().map_err(db_error)?;
        let latest_id = transaction
            .query_row(
                "SELECT c.id FROM comparisons c
                 JOIN photos left_photo ON left_photo.id = c.left_photo_id
                 JOIN photos right_photo ON right_photo.id = c.right_photo_id
                 WHERE left_photo.active = 1 AND right_photo.active = 1
                 ORDER BY c.id DESC LIMIT 1",
                [],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(db_error)?;
        let Some(latest_id) = latest_id else {
            return Ok(false);
        };

        transaction
            .execute("DELETE FROM comparisons WHERE id = ?1", [latest_id])
            .map_err(db_error)?;
        replay_all(&transaction)?;
        transaction.commit().map_err(db_error)?;
        Ok(true)
    }

    pub fn rankings(&self) -> Result<Vec<RankingRow>, String> {
        let mut photos = self.active_photos()?;
        photos.sort_by(|first, second| {
            rating::ordinal(second.mu, second.sigma)
                .total_cmp(&rating::ordinal(first.mu, first.sigma))
                .then_with(|| first.file_name.cmp(&second.file_name))
        });
        Ok(photos
            .into_iter()
            .enumerate()
            .map(|(index, photo)| RankingRow {
                rank: (index + 1) as u32,
                id: photo.id,
                file_name: photo.file_name,
                thumbnail_path: photo.thumbnail_path,
                ordinal: rating::ordinal(photo.mu, photo.sigma),
                mu: photo.mu,
                sigma: photo.sigma,
                comparison_count: photo.comparison_count,
                wins: photo.wins,
                losses: photo.losses,
                win_rate: if photo.comparison_count == 0 {
                    0.0
                } else {
                    photo.wins as f64 / photo.comparison_count as f64
                },
                sample_small: photo.comparison_count < 5,
            })
            .collect())
    }
}

fn set_photo_directory_on(connection: &Connection, directory: &str) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO settings(key, value) VALUES('photo_directory', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [directory],
        )
        .map_err(db_error)?;
    Ok(())
}

fn begin_scan_on(connection: &Connection) -> Result<(), String> {
    connection
        .execute("UPDATE photos SET active = 0", [])
        .map_err(db_error)?;
    Ok(())
}

fn active_count_on(connection: &Connection) -> Result<u32, String> {
    connection
        .query_row("SELECT COUNT(*) FROM photos WHERE active = 1", [], |row| {
            row.get(0)
        })
        .map_err(db_error)
}

fn comparison_count_on(connection: &Connection) -> Result<u32, String> {
    connection
        .query_row(
            "SELECT COUNT(*) FROM comparisons c
             JOIN photos left_photo ON left_photo.id = c.left_photo_id
             JOIN photos right_photo ON right_photo.id = c.right_photo_id
             WHERE left_photo.active = 1 AND right_photo.active = 1",
            [],
            |row| row.get(0),
        )
        .map_err(db_error)
}

#[cfg(test)]
fn set_photo_active_on(connection: &Connection, id: i64, active: bool) -> Result<(), String> {
    connection
        .execute(
            "UPDATE photos SET active = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id, active],
        )
        .map_err(db_error)?;
    Ok(())
}

fn upsert_photo_on(
    connection: &Connection,
    path: &str,
    file_name: &str,
    source_size: i64,
    source_modified_ns: i64,
) -> Result<(i64, bool, bool), String> {
    let existing = connection
        .query_row(
            "SELECT id, source_size, source_modified_ns FROM photos WHERE path = ?1",
            [path],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            },
        )
        .optional()
        .map_err(db_error)?;

    if let Some((id, old_size, old_modified_ns)) = existing {
        connection
            .execute(
                "UPDATE photos
                 SET file_name = ?2, source_size = ?3, source_modified_ns = ?4,
                     active = 1, updated_at = unixepoch()
                 WHERE id = ?1",
                params![id, file_name, source_size, source_modified_ns],
            )
            .map_err(db_error)?;
        Ok((
            id,
            false,
            old_size != source_size || old_modified_ns != source_modified_ns,
        ))
    } else {
        connection
            .execute(
                "INSERT INTO photos(path, file_name, thumbnail_path, source_size, source_modified_ns)
                 VALUES(?1, ?2, '', ?3, ?4)",
                params![path, file_name, source_size, source_modified_ns],
            )
            .map_err(db_error)?;
        Ok((connection.last_insert_rowid(), true, true))
    }
}

fn update_thumbnail_path_on(
    connection: &Connection,
    id: i64,
    thumbnail_path: &str,
) -> Result<(), String> {
    connection
        .execute(
            "UPDATE photos SET thumbnail_path = ?2, updated_at = unixepoch() WHERE id = ?1",
            params![id, thumbnail_path],
        )
        .map_err(db_error)?;
    Ok(())
}

fn photo_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PhotoRecord> {
    Ok(PhotoRecord {
        id: row.get(0)?,
        file_name: row.get(1)?,
        thumbnail_path: row.get(2)?,
        mu: row.get(3)?,
        sigma: row.get(4)?,
        comparison_count: row.get(5)?,
        wins: row.get(6)?,
        losses: row.get(7)?,
    })
}

fn apply_result(
    transaction: &Transaction<'_>,
    winner_id: i64,
    loser_id: i64,
) -> Result<(), String> {
    let winner = transaction
        .query_row(
            "SELECT mu, sigma FROM photos WHERE id = ?1",
            [winner_id],
            |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or_else(|| "找不到获胜照片".to_string())?;
    let loser = transaction
        .query_row(
            "SELECT mu, sigma FROM photos WHERE id = ?1",
            [loser_id],
            |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
        )
        .optional()
        .map_err(db_error)?
        .ok_or_else(|| "找不到落败照片".to_string())?;
    let (new_winner, new_loser) = rating::rate_win(winner.0, winner.1, loser.0, loser.1)?;

    transaction
        .execute(
            "UPDATE photos SET mu = ?2, sigma = ?3,
                comparison_count = comparison_count + 1, wins = wins + 1,
                updated_at = unixepoch() WHERE id = ?1",
            params![winner_id, new_winner.mu, new_winner.sigma],
        )
        .map_err(db_error)?;
    transaction
        .execute(
            "UPDATE photos SET mu = ?2, sigma = ?3,
                comparison_count = comparison_count + 1, losses = losses + 1,
                updated_at = unixepoch() WHERE id = ?1",
            params![loser_id, new_loser.mu, new_loser.sigma],
        )
        .map_err(db_error)?;
    Ok(())
}

fn replay_all(transaction: &Transaction<'_>) -> Result<(), String> {
    transaction
        .execute(
            "UPDATE photos SET mu = ?1, sigma = ?2, comparison_count = 0, wins = 0, losses = 0",
            params![DEFAULT_MU, DEFAULT_SIGMA],
        )
        .map_err(db_error)?;

    let events = {
        let mut statement = transaction
            .prepare(
                "SELECT left_photo_id, right_photo_id, winner_photo_id, model_version
                 FROM comparisons ORDER BY id ASC",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .map_err(db_error)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
    };

    for (left, right, winner, model_version) in events {
        if model_version != MODEL_VERSION {
            return Err(format!("无法重放评分模型 {model_version} 的比较记录"));
        }
        let loser = if winner == left { right } else { left };
        apply_result(transaction, winner, loser)?;
    }
    Ok(())
}

fn db_error(error: rusqlite::Error) -> String {
    format!("数据库操作失败：{error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seeded_database() -> Database {
        let database = Database::in_memory().unwrap();
        database.upsert_photo("a.jpg", "a.jpg", 10, 0).unwrap();
        database.upsert_photo("b.jpg", "b.jpg", 10, 0).unwrap();
        database
    }

    #[test]
    fn comparison_is_recorded_and_ranked() {
        let mut database = seeded_database();
        database.record_comparison((1, 2), 1).unwrap();

        let rankings = database.rankings().unwrap();
        assert_eq!(rankings[0].id, 1);
        assert_eq!(rankings[0].wins, 1);
        assert_eq!(rankings[1].losses, 1);
        assert_eq!(database.summary().unwrap().comparison_count, 1);
    }

    #[test]
    fn initializes_schema_and_rating_model_versions() {
        let database = Database::in_memory().unwrap();
        let schema_version: u32 = database
            .connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();

        assert_eq!(schema_version, SCHEMA_VERSION);
        assert_eq!(
            database.setting("rating_model_version").unwrap().as_deref(),
            Some(MODEL_VERSION)
        );
    }

    #[test]
    fn rejects_an_incompatible_rating_model() {
        let database = Database::in_memory().unwrap();
        database
            .connection
            .execute(
                "UPDATE settings SET value = 'other-model' WHERE key = 'rating_model_version'",
                [],
            )
            .unwrap();

        assert!(database.ensure_rating_model_compatible().is_err());
    }

    #[test]
    fn unfinished_photo_scan_rolls_back_metadata() {
        let mut database = seeded_database();
        {
            let scan = database.begin_photo_scan().unwrap();
            scan.set_photo_directory("new-directory").unwrap();
            scan.upsert_photo("c.jpg", "c.jpg", 10, 0).unwrap();
        }

        let summary = database.summary().unwrap();
        assert_eq!(summary.active_photo_count, 2);
        assert_eq!(summary.photo_directory, None);
        let new_photo_count: u32 = database
            .connection
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE path = 'c.jpg'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(new_photo_count, 0);
    }

    #[test]
    fn invalid_winner_does_not_write_an_event() {
        let mut database = seeded_database();
        assert!(database.record_comparison((1, 2), 3).is_err());
        assert_eq!(database.summary().unwrap().comparison_count, 0);
    }

    #[test]
    fn failed_rating_update_rolls_back_all_changes() {
        let mut database = seeded_database();
        database
            .connection
            .execute_batch(
                "CREATE TRIGGER reject_comparison
                 BEFORE INSERT ON comparisons
                 BEGIN
                   SELECT RAISE(FAIL, 'simulated insert failure');
                 END;",
            )
            .unwrap();
        let before = database.rankings().unwrap();
        assert!(database.record_comparison((1, 2), 1).is_err());
        let after = database.rankings().unwrap();

        assert_eq!(database.summary().unwrap().comparison_count, 0);
        assert_eq!(after[0].comparison_count, 0);
        assert_eq!(after[0].mu, before[0].mu);
    }

    #[test]
    fn inactive_photos_cannot_be_compared() {
        let mut database = seeded_database();
        database.set_photo_active(2, false).unwrap();

        assert!(database.record_comparison((1, 2), 1).is_err());
        assert!(database.pair_counts().unwrap().is_empty());
        assert_eq!(database.rankings().unwrap()[0].comparison_count, 0);
    }

    #[test]
    fn undo_replays_remaining_events() {
        let mut database = seeded_database();
        database.record_comparison((1, 2), 1).unwrap();
        let once = database.rankings().unwrap();
        database.record_comparison((1, 2), 2).unwrap();
        assert!(database.undo_last_comparison().unwrap());
        let after_undo = database.rankings().unwrap();

        assert_eq!(after_undo[0].id, once[0].id);
        assert!((after_undo[0].mu - once[0].mu).abs() < 1e-10);
        assert_eq!(database.summary().unwrap().comparison_count, 1);
    }

    #[test]
    fn incompatible_event_rolls_back_undo() {
        let mut database = seeded_database();
        database.record_comparison((1, 2), 1).unwrap();
        database.record_comparison((1, 2), 2).unwrap();
        let before = database.rankings().unwrap();
        database
            .connection
            .execute(
                "UPDATE comparisons SET model_version = 'other-model' WHERE id = 1",
                [],
            )
            .unwrap();

        assert!(database.undo_last_comparison().is_err());

        let after = database.rankings().unwrap();
        assert_eq!(database.pair_counts().unwrap(), vec![((1, 2), 2)]);
        assert_eq!(after[0].mu, before[0].mu);
        assert_eq!(after[1].mu, before[1].mu);
    }

    #[test]
    fn undo_replays_events_for_inactive_photos() {
        let mut database = seeded_database();
        database.record_comparison((1, 2), 1).unwrap();
        database.begin_scan().unwrap();
        database.upsert_photo("c.jpg", "c.jpg", 10, 0).unwrap();
        database.upsert_photo("d.jpg", "d.jpg", 10, 0).unwrap();
        database.record_comparison((3, 4), 3).unwrap();

        assert!(database.undo_last_comparison().unwrap());

        let inactive_winner_mu: f64 = database
            .connection
            .query_row("SELECT mu FROM photos WHERE id = 1", [], |row| row.get(0))
            .unwrap();
        assert!(inactive_winner_mu > DEFAULT_MU);
        assert_eq!(database.comparison_count().unwrap(), 0);
        assert_eq!(database.pair_counts().unwrap(), vec![((1, 2), 1)]);
    }
}
