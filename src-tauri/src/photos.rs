use std::{
    collections::HashSet,
    fs,
    io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use image::metadata::Orientation;
use image::{
    codecs::jpeg::JpegEncoder, DynamicImage, ExtendedColorType, ImageDecoder, ImageReader, Limits,
};
use walkdir::WalkDir;

use crate::{database::Database, models::ScanReport};

const THUMBNAIL_MAX_EDGE: u32 = 1280;
const MAX_IMAGE_DIMENSION: u32 = 16_384;
const MAX_DECODE_ALLOCATION: u64 = 256 * 1024 * 1024;
const MAX_SOURCE_FILE_SIZE: u64 = 256 * 1024 * 1024;
const THUMBNAIL_CACHE_VERSION: &str = "v2";

pub fn scan_directory(
    directory: &Path,
    database: &mut Database,
    thumbnail_directory: &Path,
) -> Result<ScanReport, String> {
    let root = directory
        .canonicalize()
        .map_err(|error| format!("无法打开照片目录：{error}"))?;
    if !root.is_dir() {
        return Err("所选路径不是文件夹".to_string());
    }
    fs::create_dir_all(thumbnail_directory)
        .map_err(|error| format!("无法创建缩略图缓存：{error}"))?;
    let thumbnail_root = thumbnail_directory
        .canonicalize()
        .map_err(|error| format!("无法打开缩略图缓存：{error}"))?;

    let mut invalid_count = 0;
    let mut candidates = Vec::new();
    let mut created_thumbnails = Vec::new();
    let walker = WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_entry(|entry| !entry.path().starts_with(&thumbnail_root));
    for entry in walker {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                invalid_count += 1;
                continue;
            }
        };
        if !entry.file_type().is_file() || !is_supported_image(entry.path()) {
            continue;
        }

        let path = match entry.path().canonicalize() {
            Ok(path) => path,
            Err(_) => {
                invalid_count += 1;
                continue;
            }
        };
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => {
                invalid_count += 1;
                continue;
            }
        };
        if metadata.len() > MAX_SOURCE_FILE_SIZE {
            invalid_count += 1;
            continue;
        }
        let (content_hash, source) = match hash_source(&path) {
            Ok(result) => result,
            Err(_) => {
                invalid_count += 1;
                continue;
            }
        };
        let thumbnail_path =
            thumbnail_directory.join(format!("{THUMBNAIL_CACHE_VERSION}-{content_hash}.jpg"));
        if !thumbnail_path.is_file() {
            let (image, orientation) = match decode_image(source) {
                Ok(image) => image,
                Err(_) => {
                    invalid_count += 1;
                    continue;
                }
            };
            if create_thumbnail(image, orientation, &thumbnail_path).is_err() {
                invalid_count += 1;
                continue;
            }
            created_thumbnails.push(thumbnail_path.clone());
        }

        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos().min(i64::MAX as u128) as i64)
            .unwrap_or_default();
        candidates.push(Candidate {
            path,
            file_name: entry.file_name().to_string_lossy().into_owned(),
            size: metadata.len().min(i64::MAX as u64) as i64,
            modified_ns,
            thumbnail_path,
        });
    }

    let scan_result = (|| {
        let scan = database.begin_photo_scan()?;
        scan.set_photo_directory(&root.to_string_lossy())?;
        let mut added_count = 0;
        for candidate in candidates {
            let path = candidate.path.to_string_lossy().into_owned();
            let (photo_id, added, _changed) = scan.upsert_photo(
                &path,
                &candidate.file_name,
                candidate.size,
                candidate.modified_ns,
            )?;
            scan.update_thumbnail_path(photo_id, &candidate.thumbnail_path.to_string_lossy())?;
            if added {
                added_count += 1;
            }
        }

        let active_photo_count = scan.active_count()?;
        let comparison_count = scan.comparison_count()?;
        let retained_thumbnails = scan.thumbnail_paths()?;
        let report = ScanReport {
            photo_directory: root.to_string_lossy().into_owned(),
            active_photo_count,
            comparison_count,
            added_count,
            invalid_count,
        };
        scan.commit()?;
        Ok((report, retained_thumbnails))
    })();
    let (report, retained_thumbnails) = match scan_result {
        Ok(result) => result,
        Err(error) => {
            for path in created_thumbnails {
                let _ = fs::remove_file(path);
            }
            return Err(error);
        }
    };
    clean_thumbnail_cache(thumbnail_directory, &retained_thumbnails);
    Ok(report)
}

#[derive(Debug)]
struct Candidate {
    path: PathBuf,
    file_name: String,
    size: i64,
    modified_ns: i64,
    thumbnail_path: PathBuf,
}

fn is_supported_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg"
            )
        })
        .unwrap_or(false)
}

fn hash_source(path: &Path) -> Result<(String, BufReader<fs::File>), String> {
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut source = BufReader::new(file);
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_read = 0_u64;
    loop {
        let read = source
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        total_read = total_read
            .checked_add(read as u64)
            .ok_or_else(|| "源文件超过大小限制".to_string())?;
        if total_read > MAX_SOURCE_FILE_SIZE {
            return Err("源文件超过大小限制".to_string());
        }
        hasher.update(&buffer[..read]);
    }
    source
        .seek(SeekFrom::Start(0))
        .map_err(|error| error.to_string())?;

    Ok((hasher.finalize().to_hex().to_string(), source))
}

fn decode_image(source: BufReader<fs::File>) -> Result<(DynamicImage, Orientation), String> {
    let mut reader = ImageReader::new(source)
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOCATION);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(|error| error.to_string())?;
    if decoder.total_bytes() > MAX_DECODE_ALLOCATION {
        return Err("解码后的图片超过内存限制".to_string());
    }
    let orientation = decoder.orientation().map_err(|error| error.to_string())?;
    let image = DynamicImage::from_decoder(decoder).map_err(|error| error.to_string())?;
    Ok((image, orientation))
}

fn create_thumbnail(
    image: DynamicImage,
    orientation: Orientation,
    destination: &Path,
) -> Result<(), String> {
    let mut thumbnail = image.thumbnail(THUMBNAIL_MAX_EDGE, THUMBNAIL_MAX_EDGE);
    thumbnail.apply_orientation(orientation);
    let thumbnail = thumbnail.to_rgb8();
    let temporary_path = destination.with_extension("jpg.tmp");
    let result = (|| {
        let output = fs::File::create(&temporary_path).map_err(|error| error.to_string())?;
        let mut writer = BufWriter::new(output);
        JpegEncoder::new_with_quality(&mut writer, 85)
            .encode(
                &thumbnail,
                thumbnail.width(),
                thumbnail.height(),
                ExtendedColorType::Rgb8,
            )
            .map_err(|error| error.to_string())?;
        writer.flush().map_err(|error| error.to_string())?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|error| error.to_string())?;
        drop(writer);
        fs::rename(&temporary_path, destination).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result
}

fn clean_thumbnail_cache(thumbnail_directory: &Path, retained_paths: &[String]) {
    let retained: HashSet<PathBuf> = retained_paths.iter().map(PathBuf::from).collect();
    let Ok(entries) = fs::read_dir(thumbnail_directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && !retained.contains(&path) {
            let _ = fs::remove_file(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
    use tempfile::tempdir;

    use super::*;
    use crate::database::Database;

    fn jpeg_with_orientation(width: u32, height: u32, orientation: u16) -> Vec<u8> {
        let image =
            DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([30, 80, 120])));
        let mut jpeg = Vec::new();
        JpegEncoder::new(&mut jpeg).encode_image(&image).unwrap();

        let mut exif = Vec::new();
        exif.extend_from_slice(b"Exif\0\0II");
        exif.extend_from_slice(&42_u16.to_le_bytes());
        exif.extend_from_slice(&8_u32.to_le_bytes());
        exif.extend_from_slice(&1_u16.to_le_bytes());
        exif.extend_from_slice(&0x0112_u16.to_le_bytes());
        exif.extend_from_slice(&3_u16.to_le_bytes());
        exif.extend_from_slice(&1_u32.to_le_bytes());
        exif.extend_from_slice(&orientation.to_le_bytes());
        exif.extend_from_slice(&0_u16.to_le_bytes());
        exif.extend_from_slice(&0_u32.to_le_bytes());

        let mut with_exif = Vec::with_capacity(jpeg.len() + exif.len() + 4);
        with_exif.extend_from_slice(&jpeg[..2]);
        with_exif.extend_from_slice(&[0xff, 0xe1]);
        with_exif.extend_from_slice(&((exif.len() + 2) as u16).to_be_bytes());
        with_exif.extend_from_slice(&exif);
        with_exif.extend_from_slice(&jpeg[2..]);
        with_exif
    }

    #[test]
    fn scans_nested_png_jpg_and_uppercase_extensions() {
        let directory = tempdir().unwrap();
        let cache_directory = tempdir().unwrap();
        let nested = directory.path().join("nested");
        fs::create_dir_all(&nested).unwrap();
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(12, 8, Rgb([30, 80, 120])));
        image
            .save_with_format(directory.path().join("one.PNG"), ImageFormat::Png)
            .unwrap();
        image
            .save_with_format(nested.join("two.jpg"), ImageFormat::Jpeg)
            .unwrap();
        fs::write(nested.join("broken.jpeg"), b"not an image").unwrap();
        fs::write(nested.join("ignore.gif"), b"not an image").unwrap();

        let mut database = Database::in_memory().unwrap();
        let report = scan_directory(
            directory.path(),
            &mut database,
            &cache_directory.path().join("thumbs"),
        )
        .unwrap();

        assert_eq!(report.active_photo_count, 2);
        assert_eq!(report.added_count, 2);
        assert_eq!(report.invalid_count, 1);
        assert_eq!(
            fs::read_dir(cache_directory.path().join("thumbs"))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn applies_jpeg_exif_orientation_to_thumbnail() {
        let directory = tempdir().unwrap();
        let cache_directory = tempdir().unwrap();
        fs::write(
            directory.path().join("portrait.jpg"),
            jpeg_with_orientation(6, 3, 6),
        )
        .unwrap();
        let mut database = Database::in_memory().unwrap();

        scan_directory(directory.path(), &mut database, cache_directory.path()).unwrap();

        let thumbnail_path = &database.active_photos().unwrap()[0].thumbnail_path;
        let thumbnail = image::open(thumbnail_path).unwrap();
        assert_eq!((thumbnail.width(), thumbnail.height()), (640, 1280));
    }

    #[test]
    fn rescanning_preserves_existing_photo_rating() {
        let directory = tempdir().unwrap();
        let cache_directory = tempdir().unwrap();
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([90, 40, 20])));
        image.save(directory.path().join("one.png")).unwrap();
        image.save(directory.path().join("two.jpg")).unwrap();
        let mut database = Database::in_memory().unwrap();
        scan_directory(
            directory.path(),
            &mut database,
            &cache_directory.path().join("thumbs"),
        )
        .unwrap();
        database.record_comparison((1, 2), 1).unwrap();
        let before = database
            .active_photos()
            .unwrap()
            .into_iter()
            .find(|photo| photo.id == 1)
            .unwrap()
            .mu;
        scan_directory(
            directory.path(),
            &mut database,
            &cache_directory.path().join("thumbs"),
        )
        .unwrap();
        let after = database
            .active_photos()
            .unwrap()
            .into_iter()
            .find(|photo| photo.id == 1)
            .unwrap()
            .mu;
        assert_eq!(after, before);
    }

    #[test]
    fn does_not_import_thumbnails_when_cache_is_inside_source() {
        let directory = tempdir().unwrap();
        let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([90, 40, 20])));
        image.save(directory.path().join("one.png")).unwrap();
        image.save(directory.path().join("two.jpg")).unwrap();
        let thumbnail_directory = directory.path().join("cache/thumbnails");
        let mut database = Database::in_memory().unwrap();

        scan_directory(directory.path(), &mut database, &thumbnail_directory).unwrap();
        let report = scan_directory(directory.path(), &mut database, &thumbnail_directory).unwrap();

        assert_eq!(report.active_photo_count, 2);
        assert_eq!(report.added_count, 0);
    }

    #[test]
    fn content_change_replaces_and_cleans_old_thumbnail() {
        let directory = tempdir().unwrap();
        let cache_directory = tempdir().unwrap();
        let source = directory.path().join("one.png");
        DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([20, 40, 60])))
            .save(&source)
            .unwrap();
        let mut database = Database::in_memory().unwrap();
        scan_directory(directory.path(), &mut database, cache_directory.path()).unwrap();
        let old_thumbnail =
            PathBuf::from(database.active_photos().unwrap()[0].thumbnail_path.clone());

        DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([180, 40, 60])))
            .save(&source)
            .unwrap();
        scan_directory(directory.path(), &mut database, cache_directory.path()).unwrap();
        let new_thumbnail =
            PathBuf::from(database.active_photos().unwrap()[0].thumbnail_path.clone());

        assert_ne!(new_thumbnail, old_thumbnail);
        assert!(new_thumbnail.is_file());
        assert!(!old_thumbnail.exists());
        assert_eq!(fs::read_dir(cache_directory.path()).unwrap().count(), 1);
    }

    #[test]
    fn skips_images_over_the_dimension_limit() {
        let directory = tempdir().unwrap();
        let cache_directory = tempdir().unwrap();
        DynamicImage::ImageRgb8(RgbImage::from_pixel(
            MAX_IMAGE_DIMENSION + 1,
            1,
            Rgb([20, 40, 60]),
        ))
        .save(directory.path().join("too-wide.png"))
        .unwrap();
        let mut database = Database::in_memory().unwrap();

        let report =
            scan_directory(directory.path(), &mut database, cache_directory.path()).unwrap();

        assert_eq!(report.active_photo_count, 0);
        assert_eq!(report.invalid_count, 1);
    }

    #[test]
    fn skips_source_files_over_the_size_limit() {
        let directory = tempdir().unwrap();
        let cache_directory = tempdir().unwrap();
        let oversized = directory.path().join("oversized.jpg");
        let file = fs::File::create(&oversized).unwrap();
        file.set_len(MAX_SOURCE_FILE_SIZE + 1).unwrap();
        let mut database = Database::in_memory().unwrap();

        let report =
            scan_directory(directory.path(), &mut database, cache_directory.path()).unwrap();

        assert_eq!(report.active_photo_count, 0);
        assert_eq!(report.invalid_count, 1);
    }

    #[test]
    fn database_failure_removes_new_thumbnail_files() {
        let directory = tempdir().unwrap();
        let cache_directory = tempdir().unwrap();
        DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([20, 40, 60])))
            .save(directory.path().join("one.png"))
            .unwrap();
        let mut database = Database::in_memory().unwrap();
        database.reject_photo_inserts_for_test().unwrap();

        assert!(scan_directory(directory.path(), &mut database, cache_directory.path(),).is_err());
        assert_eq!(fs::read_dir(cache_directory.path()).unwrap().count(), 0);
        assert_eq!(database.summary().unwrap().active_photo_count, 0);
    }
}
