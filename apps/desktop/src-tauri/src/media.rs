// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use base64::{engine::general_purpose::STANDARD, Engine as _};
use image::{imageops::FilterType, ImageFormat, ImageReader};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use tauri::State;
use uuid::Uuid;

use crate::{commands::active_session, db::AppState};

const MAX_UPLOAD_BYTES: usize = 8 * 1024 * 1024;
const MAX_PIXELS: u64 = 12_000_000;
const MAX_NORMALIZED_BYTES: usize = 900_000;
const MAX_THUMBNAIL_BYTES: usize = 80_000;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetEntityImageInput {
    pub entity_type: String,
    pub entity_id: String,
    pub image_base64: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityImage {
    pub data_url: String,
    pub updated_at: String,
}

fn permission(entity_type: &str, write: bool) -> Result<&'static str, String> {
    match (entity_type, write) {
        ("product", false) => Ok("products:read"),
        ("product", true) => Ok("products:write"),
        ("customer", false) => Ok("customers:read"),
        ("customer", true) => Ok("customers:write"),
        ("supplier", false) => Ok("suppliers:read"),
        ("supplier", true) => Ok("suppliers:write"),
        ("staff", false) => Ok("users:write"),
        ("staff", true) => Ok("users:write"),
        _ => Err("Choose a supported image type.".to_owned()),
    }
}

fn normalized_images(encoded: &str) -> Result<(Vec<u8>, Vec<u8>, u32, u32), String> {
    if encoded.len() > (MAX_UPLOAD_BYTES * 4 / 3) + 8 {
        return Err("Choose an image smaller than 8 MB.".to_owned());
    }
    let input = STANDARD
        .decode(encoded)
        .map_err(|_| "This file could not be read as an image.".to_owned())?;
    if input.is_empty() || input.len() > MAX_UPLOAD_BYTES {
        return Err("Choose an image smaller than 8 MB.".to_owned());
    }

    let reader = ImageReader::new(Cursor::new(&input))
        .with_guessed_format()
        .map_err(|_| "Choose a JPEG, PNG, or WebP image.".to_owned())?;
    let format = reader
        .format()
        .filter(|format| {
            matches!(
                format,
                ImageFormat::Jpeg | ImageFormat::Png | ImageFormat::WebP
            )
        })
        .ok_or_else(|| "Choose a JPEG, PNG, or WebP image.".to_owned())?;
    let (source_width, source_height) = reader
        .into_dimensions()
        .map_err(|_| "This image is damaged or unsupported.".to_owned())?;
    if source_width == 0
        || source_height == 0
        || u64::from(source_width) * u64::from(source_height) > MAX_PIXELS
    {
        return Err("Choose an image with no more than 12 million pixels.".to_owned());
    }

    let decoded = image::load_from_memory_with_format(&input, format)
        .map_err(|_| "This image is damaged or unsupported.".to_owned())?;
    let normalized = if decoded.width() > 960 || decoded.height() > 960 {
        decoded.resize(960, 960, FilterType::Lanczos3).to_rgb8()
    } else {
        decoded.to_rgb8()
    };
    let width = normalized.width();
    let height = normalized.height();

    let mut image_data = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut image_data, 82)
        .encode_image(&normalized)
        .map_err(|_| "This image could not be prepared for the catalog.".to_owned())?;
    if image_data.len() > MAX_NORMALIZED_BYTES {
        return Err(
            "This image is too detailed to store after resizing. Choose a smaller image."
                .to_owned(),
        );
    }

    let thumbnail = if normalized.width() > 112 || normalized.height() > 112 {
        image::imageops::thumbnail(&normalized, 112, 112)
    } else {
        normalized.clone()
    };
    let mut thumbnail_data = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut thumbnail_data, 75)
        .encode_image(&thumbnail)
        .map_err(|_| "This image thumbnail could not be prepared.".to_owned())?;
    if thumbnail_data.len() > MAX_THUMBNAIL_BYTES {
        return Err("This image thumbnail is too large to store.".to_owned());
    }

    Ok((image_data, thumbnail_data, width, height))
}

fn ensure_entity_exists(
    connection: &rusqlite::Connection,
    store_id: &str,
    entity_type: &str,
    entity_id: &str,
) -> Result<(), String> {
    let sql = match entity_type {
        "product" => "SELECT EXISTS(SELECT 1 FROM products WHERE id=?1 AND store_id=?2)",
        "customer" => "SELECT EXISTS(SELECT 1 FROM customers WHERE id=?1 AND store_id=?2)",
        "supplier" => "SELECT EXISTS(SELECT 1 FROM suppliers WHERE id=?1 AND store_id=?2)",
        "staff" => "SELECT EXISTS(SELECT 1 FROM users WHERE id=?1 AND business_id=(SELECT business_id FROM stores WHERE id=?2))",
        _ => return Err("Choose a supported image type.".to_owned()),
    };
    let exists: bool = connection
        .query_row(&sql, params![entity_id, store_id], |row| row.get(0))
        .map_err(|error| error.to_string())?;
    if exists {
        Ok(())
    } else {
        Err("That record is not available in the active store.".to_owned())
    }
}

fn record_change(
    connection: &rusqlite::Connection,
    user: &crate::models::SessionUser,
    entity_type: &str,
    entity_id: &str,
    action: &str,
) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let audit_id = Uuid::new_v4().to_string();
    let details = serde_json::json!({ "entityType": entity_type });
    connection
        .execute(
            "INSERT INTO audit_events(id,business_id,store_id,user_id,action,entity_type,entity_id,details_json,created_at)
             VALUES(?1,?2,?3,?4,?5,'entity_image',?6,?7,?8)",
            params![audit_id,user.business_id,user.store_id,user.user_id,action,entity_id,details.to_string(),now],
        )
        .map_err(|error| format!("The image was saved, but its activity record could not be written: {error}"))?;
    Ok(())
}

#[tauri::command]
pub fn set_entity_image(
    input: SetEntityImageInput,
    state: State<'_, AppState>,
) -> Result<EntityImage, String> {
    let required_permission = permission(&input.entity_type, true)?;
    let active = active_session(&state, required_permission)?;
    if input.entity_type == "staff" && !active.user.is_owner && active.user.role_key != "admin" {
        return Err(
            "Only the business owner or an administrator can manage staff profile photos."
                .to_owned(),
        );
    }
    let (image_data, thumbnail_data, width, height) = normalized_images(&input.image_base64)?;
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    ensure_entity_exists(
        &connection,
        &active.store_id,
        &input.entity_type,
        &input.entity_id,
    )?;
    let updated_at = chrono::Utc::now().to_rfc3339();
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    if input.entity_type == "staff" {
        let mut stores = transaction
            .prepare("SELECT id FROM stores WHERE business_id=?1")
            .map_err(|error| error.to_string())?;
        let store_ids = stores
            .query_map([&active.user.business_id], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        drop(stores);
        for store_id in store_ids {
            transaction
                .execute(
                    "INSERT INTO entity_images(store_id,entity_type,entity_id,image_data,thumbnail_data,width,height,updated_at)
                     VALUES(?1,'staff',?2,?3,?4,?5,?6,?7)
                     ON CONFLICT(store_id,entity_type,entity_id) DO UPDATE SET
                       image_data=excluded.image_data,thumbnail_data=excluded.thumbnail_data,
                       width=excluded.width,height=excluded.height,updated_at=excluded.updated_at",
                    params![store_id,input.entity_id,&image_data,&thumbnail_data,width,height,updated_at],
                )
                .map_err(|error| format!("Could not save this image: {error}"))?;
        }
    } else {
        transaction
            .execute(
                "INSERT INTO entity_images(store_id,entity_type,entity_id,image_data,thumbnail_data,width,height,updated_at)
                 VALUES(?1,?2,?3,?4,?5,?6,?7,?8)
                 ON CONFLICT(store_id,entity_type,entity_id) DO UPDATE SET
                   image_data=excluded.image_data,thumbnail_data=excluded.thumbnail_data,
                   width=excluded.width,height=excluded.height,updated_at=excluded.updated_at",
                params![active.store_id,input.entity_type,input.entity_id,image_data,thumbnail_data,width,height,updated_at],
            )
            .map_err(|error| format!("Could not save this image: {error}"))?;
    }
    record_change(
        &transaction,
        &active.user,
        &input.entity_type,
        &input.entity_id,
        "entity_image.updated",
    )?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(EntityImage {
        data_url: format!("data:image/jpeg;base64,{}", STANDARD.encode(image_data)),
        updated_at,
    })
}

#[tauri::command]
pub fn get_entity_image(
    entity_type: String,
    entity_id: String,
    state: State<'_, AppState>,
) -> Result<Option<EntityImage>, String> {
    let required_permission = permission(&entity_type, false)?;
    let active = active_session(&state, required_permission)?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    ensure_entity_exists(&connection, &active.store_id, &entity_type, &entity_id)?;
    let image = connection
        .query_row(
            "SELECT image_data,updated_at FROM entity_images WHERE store_id=?1 AND entity_type=?2 AND entity_id=?3",
            params![active.store_id,entity_type,entity_id],
            |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(image.map(|(bytes, updated_at)| EntityImage {
        data_url: format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)),
        updated_at,
    }))
}

#[tauri::command]
pub fn get_entity_image_thumbnail(
    entity_type: String,
    entity_id: String,
    state: State<'_, AppState>,
) -> Result<Option<String>, String> {
    let required_permission = permission(&entity_type, false)?;
    let active = active_session(&state, required_permission)?;
    let connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    ensure_entity_exists(&connection, &active.store_id, &entity_type, &entity_id)?;
    let thumbnail = connection
        .query_row(
            "SELECT thumbnail_data FROM entity_images WHERE store_id=?1 AND entity_type=?2 AND entity_id=?3",
            params![active.store_id,entity_type,entity_id],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    Ok(thumbnail.map(|bytes| format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))))
}

#[tauri::command]
pub fn delete_entity_image(
    entity_type: String,
    entity_id: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let required_permission = permission(&entity_type, true)?;
    let active = active_session(&state, required_permission)?;
    if entity_type == "staff" && !active.user.is_owner && active.user.role_key != "admin" {
        return Err(
            "Only the business owner or an administrator can manage staff profile photos."
                .to_owned(),
        );
    }
    let mut connection = state
        .db
        .lock()
        .map_err(|_| "The local database is unavailable.".to_owned())?;
    ensure_entity_exists(&connection, &active.store_id, &entity_type, &entity_id)?;
    let transaction = connection
        .transaction()
        .map_err(|error| error.to_string())?;
    let deleted = if entity_type == "staff" {
        transaction
            .execute(
                "DELETE FROM entity_images WHERE entity_type='staff' AND entity_id=?1 AND store_id IN (SELECT id FROM stores WHERE business_id=?2)",
                params![entity_id, active.user.business_id],
            )
            .map_err(|error| error.to_string())?
    } else {
        transaction
            .execute(
                "DELETE FROM entity_images WHERE store_id=?1 AND entity_type=?2 AND entity_id=?3",
                params![active.store_id, entity_type, entity_id],
            )
            .map_err(|error| error.to_string())?
    };
    if deleted > 0 {
        record_change(
            &transaction,
            &active.user,
            &entity_type,
            &entity_id,
            "entity_image.deleted",
        )?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageBuffer, Rgb};

    #[test]
    fn image_upload_is_normalized_to_bounded_jpeg_and_thumbnail() {
        let source = ImageBuffer::from_fn(40, 24, |x, y| Rgb([x as u8, y as u8, 120]));
        let mut png = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(source)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();

        let (full, thumbnail, width, height) =
            normalized_images(&STANDARD.encode(png.into_inner())).unwrap();
        assert_eq!((width, height), (40, 24));
        assert!(full.len() <= MAX_NORMALIZED_BYTES);
        assert!(thumbnail.len() <= MAX_THUMBNAIL_BYTES);
        assert!(full.starts_with(&[0xff, 0xd8]));
        assert!(thumbnail.starts_with(&[0xff, 0xd8]));
    }

    #[test]
    fn image_upload_rejects_svg_and_malformed_base64() {
        assert!(normalized_images(
            &STANDARD.encode(b"<svg xmlns='http://www.w3.org/2000/svg'></svg>")
        )
        .is_err());
        assert!(normalized_images("not base64").is_err());
    }

    #[test]
    fn entity_image_migration_creates_asset_table_and_removes_deleted_entity_images() {
        let connection = rusqlite::Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "PRAGMA foreign_keys=ON;
             CREATE TABLE stores(id TEXT PRIMARY KEY);
             CREATE TABLE products(id TEXT PRIMARY KEY, store_id TEXT NOT NULL);
             CREATE TABLE customers(id TEXT PRIMARY KEY, store_id TEXT NOT NULL);
             CREATE TABLE suppliers(id TEXT PRIMARY KEY, store_id TEXT NOT NULL);
             CREATE TABLE local_schema_migrations(version INTEGER PRIMARY KEY);
             CREATE TABLE audit_events(id TEXT PRIMARY KEY, business_id TEXT, store_id TEXT, user_id TEXT,
                 action TEXT, entity_type TEXT, entity_id TEXT, details_json TEXT, created_at TEXT);
             INSERT INTO stores VALUES('store-a');
             INSERT INTO products VALUES('product-a','store-a');",
        )
        .unwrap();
        connection
            .execute_batch(include_str!("../migrations/0033_entity_images.sql"))
            .unwrap();
        connection
            .execute(
                "INSERT INTO entity_images(store_id,entity_type,entity_id,image_data,thumbnail_data,width,height,updated_at)
                 VALUES('store-a','product','product-a',X'00',X'01',1,1,'now')",
                [],
            )
            .unwrap();

        connection
            .execute("DELETE FROM products WHERE id='product-a'", [])
            .unwrap();
        let remaining: i64 = connection
            .query_row("SELECT COUNT(*) FROM entity_images", [], |row| row.get(0))
            .unwrap();
        let version: i64 = connection
            .query_row("SELECT version FROM local_schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(version, 33);
    }
}
