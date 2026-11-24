use crate::api::dto::{
    ConvertImageResultDto, JobEventDto, JobFailureDto, JobProgressDto, QueueSummaryDto,
};
use crate::core::conversion::pipeline::ConversionPipeline;
use crate::core::conversion::registry::ConverterRegistry;
use crate::core::jobs::events::{CoreJobEvent, JobBroadcaster};
use crate::core::jobs::JobManager;
use std::sync::Arc;
use tauri::Manager;

pub mod api;
pub mod context_menu;
pub mod i18n;
/// The conversion engine lives in its own crate (`crates/zeroconverter-core`);
/// `crate::core::...` paths in the app keep working through this name.
pub use zeroconverter_core as core;
pub mod receipt;
pub mod rename_convert;
pub mod settings;
pub mod tray;
pub mod updater;

pub struct AppState {
    pub pipeline: Arc<ConversionPipeline>,
    pub jobs: Arc<JobManager>,
}

/// Convert Core-domain events to API-transport DTOs.
/// This is the ONLY place where Core types are mapped to API types.
fn map_core_event_to_dto(event: &CoreJobEvent) -> serde_json::Value {
    let dto: JobEventDto = match event {
        CoreJobEvent::JobStarted { id } => JobEventDto::JobStarted { id: id.clone() },
        CoreJobEvent::StatusChanged { id, status } => JobEventDto::JobStatusChanged {
            id: id.clone(),
            status: status.clone(),
        },
        CoreJobEvent::ProgressUpdated { id, progress } => JobEventDto::JobProgressUpdated {
            id: id.clone(),
            progress: JobProgressDto {
                stage: progress.stage.clone(),
                message: progress.message.clone(),
                percent: progress.percent,
            },
        },
        CoreJobEvent::Completed { id, result } => JobEventDto::JobCompleted {
            id: id.clone(),
            result: ConvertImageResultDto::from(result.clone()),
        },
        CoreJobEvent::Failed { id, failure } => JobEventDto::JobFailed {
            id: id.clone(),
            failure: JobFailureDto {
                stage: format!("{:?}", failure.stage),
                kind: format!("{:?}", failure.kind),
                message: failure.message.clone(),
            },
        },
        CoreJobEvent::QueueSummaryChanged { summary } => JobEventDto::QueueSummaryChanged {
            summary: QueueSummaryDto::from(summary.clone()),
        },
    };
    serde_json::to_value(dto).unwrap_or_default()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Image, audio and video (FFmpeg, downloaded on first use): the same set
    // the command line uses.
    let registry = Arc::new(ConverterRegistry::standard());
    let pipeline = Arc::new(ConversionPipeline::new(Arc::clone(&registry)));
    let rename_convert = crate::rename_convert::RenameConvert::new(Arc::clone(&registry));

    // The uninstaller runs this to take the right-click menu away.
    if std::env::args().any(|a| a == crate::context_menu::UNREGISTER_ARG) {
        let _ = crate::context_menu::unregister_everywhere(&rename_convert.known_extensions());
        return;
    }

    // Create broadcaster with the mapper that converts Core events to API DTOs
    let broadcaster = Arc::new(JobBroadcaster::new(Box::new(map_core_event_to_dto)));
    let jobs = Arc::new(JobManager::new(broadcaster));
    let state = AppState { pipeline, jobs };

    tauri::Builder::default()
        // A second launch (start menu, autostart) just brings this one forward.
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            // A right-click menu item: convert, stay in the tray.
            if let Some((target, file)) = crate::context_menu::convert_request(&args) {
                crate::rename_convert::state(app).convert_copy(app, file, target);
                return;
            }
            crate::tray::show_main(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .manage(crate::updater::Pending::default())
        .manage(Arc::clone(&rename_convert))
        .setup(move |app| {
            let handle = app.handle().clone();
            if let Ok(dir) = handle.path().app_data_dir() {
                crate::core::handlers::video::ffmpeg::set_data_dir(dir);
            }
            let settings = crate::settings::load(&handle);
            // The window tells us its language once it runs; until then the
            // saved one, or the Windows display language.
            if settings.language.is_empty() || !crate::i18n::set(&settings.language) {
                crate::i18n::set(crate::i18n::system_language());
            }
            rename_convert.enabled.store(
                settings.rename_convert,
                std::sync::atomic::Ordering::Relaxed,
            );
            rename_convert
                .keep_original
                .store(settings.keep_original, std::sync::atomic::Ordering::Relaxed);
            rename_convert.start(handle.clone());
            crate::updater::start(&handle);
            // Also when off: a menu left over from before is taken away.
            api::rename_commands::sync_context_menu(&handle);
            let args: Vec<String> = std::env::args().collect();
            let from_menu = crate::context_menu::convert_request(&args);
            crate::tray::create(&handle)?;
            crate::receipt::init(&handle)?;
            // Launched by Windows at sign-in, or by the right-click menu: stay
            // in the tray.
            if let Some((target, file)) = from_menu {
                rename_convert.convert_copy(&handle, file, target);
            } else if !args.iter().any(|a| a == "--background") {
                crate::tray::show_main(&handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // An options window closed any way (X, Alt+F4) counts as Cancel:
            // the file gets its old name back. After Convert the job has
            // already left the waiting list, so this does nothing.
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(id) = window.label().strip_prefix("quick-") {
                    crate::rename_convert::state(window.app_handle()).quick_cancel(id);
                }
            }
            // Closing the main window keeps rename-to-convert running in the tray.
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                    let app = window.app_handle();
                    if !crate::settings::load(app).tray_hint_shown {
                        let _ = crate::settings::save(app, |s| s.tray_hint_shown = true);
                        use tauri_plugin_notification::NotificationExt;
                        let _ = app
                            .notification()
                            .builder()
                            .title(crate::i18n::tr("stillRunning.title", &[]))
                            .body(crate::i18n::tr("stillRunning.body", &[]))
                            .show();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            api::bridge::get_app_status,
            api::bridge::get_app_capabilities,
            api::bridge::attach_job_events,
            api::bridge::submit_conversion_batch,
            api::bridge::preflight_conversion,
            api::bridge::get_queue_status,
            api::bridge::list_jobs,
            api::bridge::get_job_status,
            api::bridge::cancel_job,
            api::bridge::purge_jobs,
            api::bridge::get_files_metadata,
            api::bridge::open_completed_output,
            api::thumbnail_commands::get_or_create_thumbnail,
            api::rename_commands::rename_convert_status,
            api::language_commands::set_ui_language,
            api::rename_commands::rename_convert_set_enabled,
            api::rename_commands::rename_convert_set_keep_original,
            api::rename_commands::rename_convert_set_autostart,
            api::rename_commands::rename_convert_add_folder,
            api::rename_commands::rename_convert_remove_folder,
            api::rename_commands::context_menu_set,
            api::rename_commands::quick_job_info,
            api::rename_commands::quick_job_confirm,
            api::rename_commands::quick_job_cancel,
            api::receipt_commands::notification_prefs,
            api::receipt_commands::set_notification_prefs,
            api::receipt_commands::preview_notification,
            api::receipt_commands::show_receipt,
            api::receipt_commands::receipt_current,
            api::receipt_commands::receipt_painted,
            api::ffmpeg_commands::ffmpeg_status,
            api::ffmpeg_commands::ffmpeg_install,
            api::ffmpeg_commands::ffmpeg_install_cancel,
            api::ffmpeg_commands::take_settings_request,
            api::update_commands::update_status,
            api::update_commands::update_set_auto,
            api::update_commands::update_check,
            api::update_commands::update_install,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
