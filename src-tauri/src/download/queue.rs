use crate::download::orchestrator::handle_download_task;
use crate::download::types::DownloadTask;
use tauri::AppHandle;
use tokio::sync::mpsc;

pub struct DownloadQueue {
    pub tx: mpsc::UnboundedSender<DownloadTask>,
}

pub fn init_queue_worker(app: AppHandle) -> mpsc::UnboundedSender<DownloadTask> {
    let (tx, mut rx) = mpsc::unbounded_channel::<DownloadTask>();

    tauri::async_runtime::spawn(async move {
        println!("[Queue Worker] Core pipeline active. Awaiting jobs...");

        while let Some(task) = rx.recv().await {
            if let Err(e) = handle_download_task(task, app.clone()).await {
                eprintln!("[Queue Worker] Task failed: {e}");
            }
        }
    });

    tx
}
