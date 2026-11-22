// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2026 Powered Solutions

use serde::Serialize;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{mpsc, Mutex};
use tauri::{AppHandle, Emitter, State};
use velopack::{
    locator::{auto_locate_app_manifest, LocationContext},
    sources::{HttpSource, UpdateSource},
    UpdateCheck, UpdateInfo, UpdateManager, VelopackAsset,
};

pub struct UpdateState {
    manager: Option<UpdateManager>,
    source: Option<HttpSource>,
    error: Option<String>,
    pending: Mutex<Option<UpdateInfo>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    configured: bool,
    installed: bool,
    available: bool,
    downloaded: bool,
    version: Option<String>,
    notes: Option<String>,
    delta_available: bool,
    download_size_bytes: u64,
    message: Option<String>,
}

impl UpdateState {
    pub fn new() -> Self {
        let source = configured_feed_url();
        match source {
            Some(url) => {
                let source = HttpSource::new(url);
                match UpdateManager::new(source.clone(), None, None) {
                    Ok(manager) => Self {
                        manager: Some(manager),
                        source: Some(source),
                        error: None,
                        pending: Mutex::new(None),
                    },
                    Err(error) => Self {
                        manager: None,
                        source: None,
                        error: Some(error.to_string()),
                        pending: Mutex::new(None),
                    },
                }
            }
            None => Self {
                manager: None,
                source: None,
                error: Some(
                    "The HTTPS desktop update feed is not configured in this build.".to_owned(),
                ),
                pending: Mutex::new(None),
            },
        }
    }
}

fn configured_feed_url() -> Option<&'static str> {
    option_env!("ADVANCED_POS_UPDATE_FEED_URL")
        .map(str::trim)
        .filter(|url| url.starts_with("https://"))
}

#[tauri::command]
pub async fn check_for_update(state: State<'_, UpdateState>) -> Result<UpdateStatus, String> {
    let Some(manager) = state.manager.clone() else {
        return Ok(UpdateStatus {
            configured: configured_feed_url().is_some(),
            installed: false,
            available: false,
            downloaded: false,
            version: None,
            notes: None,
            delta_available: false,
            download_size_bytes: 0,
            message: state.error.clone(),
        });
    };
    let check_manager = manager.clone();
    let check = tauri::async_runtime::spawn_blocking(move || check_manager.check_for_updates())
        .await
        .map_err(|error| format!("Update check failed: {error}"))?
        .map_err(|error| format!("Update check failed: {error}"))?;
    match check {
        UpdateCheck::UpdateAvailable(info) => {
            let downloaded = manager.get_update_pending_restart().is_some_and(|asset| {
                auto_locate_app_manifest(LocationContext::FromCurrentExe)
                    .ok()
                    .and_then(|locator| package_path(&locator.get_packages_dir(), &asset).ok())
                    .is_some_and(|path| verify_reconstructed_package(&path, &asset).is_ok())
            });
            let delta_available = !info.DeltasToTarget.is_empty();
            let download_size_bytes = if delta_available {
                info.DeltasToTarget.iter().map(|asset| asset.Size).sum()
            } else {
                info.TargetFullRelease.Size
            };
            let response = UpdateStatus {
                configured: true,
                installed: true,
                available: true,
                downloaded,
                version: Some(info.TargetFullRelease.Version.clone()),
                notes: Some(info.TargetFullRelease.NotesMarkdown.clone()),
                delta_available,
                download_size_bytes,
                message: None,
            };
            *state
                .pending
                .lock()
                .map_err(|_| "Update state is unavailable.".to_owned())? = Some(*info);
            Ok(response)
        }
        UpdateCheck::NoUpdateAvailable | UpdateCheck::RemoteIsEmpty => Ok(UpdateStatus {
            configured: true,
            installed: true,
            available: false,
            downloaded: false,
            version: None,
            notes: None,
            delta_available: false,
            download_size_bytes: 0,
            message: None,
        }),
    }
}

#[tauri::command]
pub async fn download_update(app: AppHandle, state: State<'_, UpdateState>) -> Result<(), String> {
    let _manager = state.manager.clone().ok_or_else(|| {
        state
            .error
            .clone()
            .unwrap_or_else(|| "Desktop updates are unavailable.".to_owned())
    })?;
    let update = state
        .pending
        .lock()
        .map_err(|_| "Update state is unavailable.".to_owned())?
        .clone()
        .ok_or_else(|| "Check for an update before downloading.".to_owned())?;
    let source = state.source.clone().ok_or_else(|| {
        state
            .error
            .clone()
            .unwrap_or_else(|| "Desktop updates are unavailable.".to_owned())
    })?;
    if update.IsDowngrade || update.BaseRelease.is_none() || update.DeltasToTarget.is_empty() {
        return Err("A delta update is not available for this installation. No full application package was downloaded. Contact support for a one-time migration or recovery package.".to_owned());
    }
    let (progress_tx, progress_rx) = mpsc::channel();
    let worker = tauri::async_runtime::spawn_blocking(move || {
        download_delta_update(&source, &update, Some(progress_tx))
    });
    let progress_app = app;
    let progress_worker = tauri::async_runtime::spawn_blocking(move || {
        while let Ok(percent) = progress_rx.recv() {
            let _ = progress_app.emit("desktop-update-progress", percent.clamp(0, 100));
        }
    });
    worker
        .await
        .map_err(|error| format!("Update download failed: {error}"))?
        .map_err(|error| format!("Update download failed: {error}"))?;
    progress_worker
        .await
        .map_err(|error| format!("Update progress stream failed: {error}"))?;
    Ok(())
}

/// Download and reconstruct the target exclusively from delta packages.
/// Any missing/invalid patch fails closed: this function never requests the full bundle.
fn download_delta_update(
    source: &HttpSource,
    update: &UpdateInfo,
    progress: Option<mpsc::Sender<i16>>,
) -> Result<(), String> {
    if update.IsDowngrade || update.DeltasToTarget.is_empty() {
        return Err("This release has no usable delta chain. The full application package was not downloaded.".to_owned());
    }
    let locator = auto_locate_app_manifest(LocationContext::FromCurrentExe)
        .map_err(|error| format!("Could not locate the installed application: {error}"))?;
    let _lock = locator
        .try_get_exclusive_lock()
        .map_err(|error| format!("Another update is already running: {error}"))?;
    let packages_dir = locator.get_packages_dir();
    fs::create_dir_all(&packages_dir)
        .map_err(|error| format!("Could not prepare update storage: {error}"))?;
    let base = update
        .BaseRelease
        .as_ref()
        .ok_or_else(|| "The delta chain has no base release.".to_owned())?;
    let base_path = installed_base_package_path(&packages_dir, base)?;
    if !base_path.is_file() {
        return Err("The installed base package required for this delta is missing. No full application package was downloaded.".to_owned());
    }
    verify_asset(&base_path, base)?;

    let output_path = package_path(&packages_dir, &update.TargetFullRelease)?;
    if output_path.is_file() {
        if verify_reconstructed_package(&output_path, &update.TargetFullRelease).is_ok() {
            return Ok(());
        }
        fs::remove_file(&output_path)
            .map_err(|error| format!("Could not remove an invalid staged update: {error}"))?;
    }
    let partial_output = output_path.with_extension("partial");
    let _ = fs::remove_file(&partial_output);
    let mut patch_args = vec![
        "patch".to_owned(),
        "--old".to_owned(),
        base_path.to_string_lossy().into_owned(),
        "--output".to_owned(),
        partial_output.to_string_lossy().into_owned(),
    ];
    let delta_count = update.DeltasToTarget.len();
    for (index, delta) in update.DeltasToTarget.iter().enumerate() {
        let delta_path = package_path(&packages_dir, delta)?;
        if delta_path.is_file() && verify_asset(&delta_path, delta).is_ok() {
            patch_args.push("--delta".to_owned());
            patch_args.push(delta_path.to_string_lossy().into_owned());
            continue;
        }
        let _ = fs::remove_file(&delta_path);
        let partial_delta = delta_path.with_extension("partial");
        let _ = fs::remove_file(&partial_delta);
        let delta_progress = progress.as_ref().map(|sender| {
            let start = index as f32 / delta_count as f32 * 70.0;
            let end = (index + 1) as f32 / delta_count as f32 * 70.0;
            let sender = sender.clone();
            move |percent: i16| {
                let scaled = start + (end - start) * f32::from(percent.clamp(0, 100)) / 100.0;
                let _ = sender.send(scaled.round() as i16);
            }
        });
        let delta_tx = delta_progress.map(|callback| {
            let (tx, rx) = mpsc::channel();
            std::thread::spawn(move || {
                while let Ok(percent) = rx.recv() {
                    callback(percent);
                }
            });
            tx
        });
        source
            .download_release_entry(delta, &partial_delta, delta_tx)
            .map_err(|error| format!("Could not download delta {}: {error}", delta.Version))?;
        verify_asset(&partial_delta, delta)?;
        fs::rename(&partial_delta, &delta_path)
            .map_err(|error| format!("Could not stage delta {}: {error}", delta.Version))?;
        patch_args.push("--delta".to_owned());
        patch_args.push(delta_path.to_string_lossy().into_owned());
    }

    if let Some(sender) = &progress {
        let _ = sender.send(70);
    }
    let patch = Command::new(locator.get_update_path())
        .args(patch_args)
        .output()
        .map_err(|error| format!("Could not apply the downloaded delta patches: {error}"))?;
    if !patch.status.success() {
        let _ = fs::remove_file(&partial_output);
        return Err(format!(
            "Delta patch reconstruction failed; no full package was downloaded. {}",
            String::from_utf8_lossy(&patch.stderr).trim()
        ));
    }
    verify_reconstructed_package(&partial_output, &update.TargetFullRelease)?;
    fs::rename(&partial_output, &output_path)
        .map_err(|error| format!("Could not stage the reconstructed update: {error}"))?;
    if let Some(sender) = progress {
        let _ = sender.send(100);
    }
    Ok(())
}

fn package_path(packages_dir: &Path, asset: &VelopackAsset) -> Result<PathBuf, String> {
    let filename = Path::new(&asset.FileName)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| "The update feed contains an invalid package filename.".to_owned())?;
    if filename != asset.FileName
        || filename == "."
        || filename == ".."
        || !filename.ends_with(".nupkg")
        || filename.contains('/')
        || filename.contains('\\')
        || filename.contains(':')
    {
        return Err("The update feed contains an unsafe package filename.".to_owned());
    }
    Ok(packages_dir.join(filename))
}

fn installed_base_package_path(
    packages_dir: &Path,
    asset: &VelopackAsset,
) -> Result<PathBuf, String> {
    let feed_path = package_path(packages_dir, asset)?;
    if feed_path.is_file() || !asset.Type.eq_ignore_ascii_case("full") {
        return Ok(feed_path);
    }

    // Setup.exe stores installed releases under Velopack's canonical local name,
    // without the channel segment used in published feed asset filenames.
    let local_asset = VelopackAsset {
        FileName: format!("{}-{}-full.nupkg", asset.PackageId, asset.Version),
        ..asset.clone()
    };
    package_path(packages_dir, &local_asset)
}

fn verify_reconstructed_package(path: &Path, asset: &VelopackAsset) -> Result<(), String> {
    if !asset.Type.eq_ignore_ascii_case("full") {
        return Err("The update feed target is not a full application package.".to_owned());
    }
    let mut package = velopack::bundle::load_bundle_from_file(path)
        .map_err(|error| format!("Could not open reconstructed update package: {error}"))?;
    let manifest = package
        .read_manifest()
        .map_err(|error| format!("Could not read reconstructed update manifest: {error}"))?;
    if manifest.id != asset.PackageId || manifest.version.to_string() != asset.Version {
        return Err(
            "The reconstructed package identity or version does not match the update feed."
                .to_owned(),
        );
    }
    let expected_main_exe = format!("lib/app/{}", manifest.main_exe.replace('\\', "/"));
    let package_files = package
        .get_file_names()
        .map_err(|error| format!("Could not inspect reconstructed update files: {error}"))?;
    if manifest.main_exe.is_empty()
        || !package_files
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&expected_main_exe))
    {
        return Err(
            "The reconstructed package is missing its declared application executable.".to_owned(),
        );
    }
    Ok(())
}

fn verify_asset(path: &Path, asset: &VelopackAsset) -> Result<(), String> {
    let metadata = fs::metadata(path)
        .map_err(|error| format!("Could not inspect downloaded package: {error}"))?;
    if metadata.len() != asset.Size {
        return Err(format!(
            "Package size verification failed for {}.",
            asset.FileName
        ));
    }
    let mut file =
        File::open(path).map_err(|error| format!("Could not read downloaded package: {error}"))?;
    let mut sha1 = Sha1::new();
    let mut sha256 = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("Could not verify downloaded package: {error}"))?;
        if count == 0 {
            break;
        }
        sha1.update(&buffer[..count]);
        sha256.update(&buffer[..count]);
    }
    let actual_sha256 = format!("{:x}", sha256.finalize());
    let actual_sha1 = format!("{:x}", sha1.finalize());
    let valid = if asset.SHA256.is_empty() {
        actual_sha1.eq_ignore_ascii_case(&asset.SHA1)
    } else {
        actual_sha256.eq_ignore_ascii_case(&asset.SHA256)
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Package checksum verification failed for {}.",
            asset.FileName
        ))
    }
}

#[tauri::command]
pub fn apply_downloaded_update(state: State<'_, UpdateState>) -> Result<(), String> {
    let manager = state.manager.as_ref().ok_or_else(|| {
        state
            .error
            .clone()
            .unwrap_or_else(|| "Desktop updates are unavailable.".to_owned())
    })?;
    let update = manager
        .get_update_pending_restart()
        .ok_or_else(|| "Download the update before restarting to apply it.".to_owned())?;
    manager
        .apply_updates_and_restart(&update)
        .map_err(|error| format!("Could not apply the update: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset(file_name: &str) -> VelopackAsset {
        VelopackAsset {
            PackageId: "PoweredSolutions.AdvancedPOS".to_owned(),
            Version: "2.0.1".to_owned(),
            Type: "Full".to_owned(),
            FileName: file_name.to_owned(),
            ..VelopackAsset::default()
        }
    }

    #[test]
    fn installed_base_uses_canonical_velopack_name_when_feed_name_has_channel() {
        let packages_dir = std::env::temp_dir().join(format!(
            "advanced-pos-updater-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&packages_dir).unwrap();
        let local_package = packages_dir.join("PoweredSolutions.AdvancedPOS-2.0.1-full.nupkg");
        fs::write(&local_package, b"installed base package").unwrap();
        let feed_asset = asset("PoweredSolutions.AdvancedPOS-2.0.1-stable-full.nupkg");

        let resolved = installed_base_package_path(&packages_dir, &feed_asset).unwrap();

        assert_eq!(resolved, local_package);
        fs::remove_dir_all(packages_dir).unwrap();
    }

    #[test]
    fn installed_base_rejects_feed_path_traversal() {
        let packages_dir = std::env::temp_dir().join(format!(
            "advanced-pos-updater-test-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&packages_dir).unwrap();
        let feed_asset = asset("../PoweredSolutions.AdvancedPOS-2.0.1-full.nupkg");

        assert!(installed_base_package_path(&packages_dir, &feed_asset).is_err());
        fs::remove_dir_all(packages_dir).unwrap();
    }
}
