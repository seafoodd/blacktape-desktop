use crate::download::orchestrator::handle_download_task;
use crate::download::types::DownloadTask;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio::sync::{mpsc, Mutex};
use tokio::task::AbortHandle;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackProgressState {
    pub title: String,
    pub track_number: Option<i32>,
    pub percent: i32,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveDownloadState {
    pub id: String,
    pub platform: String,
    pub download_type: String,
    pub url: String,
    pub artist_name: String,
    pub album_title: String,
    pub cover_url: Option<String>,
    pub status: String,
    pub current: usize,
    pub total: usize,
    pub overall_percent: i32,
    pub message: String,
    pub tracks: Vec<TrackProgressState>,
}

pub struct ActiveTaskInfo {
    pub abort_handle: Option<AbortHandle>,
    pub temp_dir: Option<PathBuf>,
    pub target_dir: Option<PathBuf>,
    pub root_dir: Option<PathBuf>,
    pub child_pids: Vec<u32>,
    pub is_cancelled: bool,
    /// yt-dlp format id the user picked in the quality menu (tried first; the platform's chain is the fallback).
    pub format: Option<String>,
    pub state: ActiveDownloadState,
}

#[derive(Clone, Default)]
pub struct TaskRegistry {
    pub tasks: Arc<Mutex<HashMap<String, ActiveTaskInfo>>>,
    pub cancelled_tasks: Arc<Mutex<HashSet<String>>>,
}

impl TaskRegistry {
    pub fn kill_pid(pid: u32) {
        println!("[Process Kill] Killing yt-dlp PID: {pid}");
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            let output = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .creation_flags(0x08000000)
                .output();
            if let Ok(out) = output {
                println!(
                    "[Process Kill] taskkill for PID {} exited with {}. Out: {} Err: {}",
                    pid,
                    out.status,
                    String::from_utf8_lossy(&out.stdout).trim(),
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
    }

    pub async fn mark_cancelled(&self, task_id: &str) {
        self.cancelled_tasks
            .lock()
            .await
            .insert(task_id.to_string());
        let (pids, handle) = {
            let mut tasks = self.tasks.lock().await;
            match tasks.get_mut(task_id) {
                Some(info) => {
                    info.is_cancelled = true;
                    (std::mem::take(&mut info.child_pids), info.abort_handle.take())
                }
                None => (Vec::new(), None),
            }
        };

        // Kill process trees BEFORE aborting the future: aborting drops the Child
        // (kill_on_drop) which kills only the parent, after which `taskkill /T`
        // can no longer walk the tree and grandchildren keep files locked.
        if !pids.is_empty() {
            let _ = tokio::task::spawn_blocking(move || {
                for pid in pids {
                    Self::kill_pid(pid);
                }
            })
            .await;
        }

        if let Some(handle) = handle {
            handle.abort();
        }
    }

    pub async fn is_cancelled(&self, task_id: &str) -> bool {
        if self.cancelled_tasks.lock().await.contains(task_id) {
            return true;
        }
        let tasks = self.tasks.lock().await;
        tasks.get(task_id).map(|t| t.is_cancelled).unwrap_or(false)
    }

    pub async fn register_task(&self, state: ActiveDownloadState) {
        let mut tasks = self.tasks.lock().await;
        tasks.insert(
            state.id.clone(),
            ActiveTaskInfo {
                abort_handle: None,
                temp_dir: None,
                target_dir: None,
                root_dir: None,
                child_pids: Vec::new(),
                is_cancelled: false,
                format: None,
                state,
            },
        );
    }

    pub async fn register_pid(&self, task_id: &str, pid: u32) {
        if self.is_cancelled(task_id).await {
            println!(
                "[Process Kill] Task {} already cancelled, terminating newly registered PID {}",
                task_id, pid
            );
            Self::kill_pid(pid);
            return;
        }

        let mut tasks = self.tasks.lock().await;
        if let Some(info) = tasks.get_mut(task_id) {
            if info.is_cancelled {
                drop(tasks);
                Self::kill_pid(pid);
                return;
            }
            if !info.child_pids.contains(&pid) {
                info.child_pids.push(pid);
            }
        }
    }

    pub async fn unregister_pid(&self, task_id: &str, pid: u32) {
        let mut tasks = self.tasks.lock().await;
        if let Some(info) = tasks.get_mut(task_id) {
            info.child_pids.retain(|&p| p != pid);
        }
    }

    pub async fn kill_pids_for_task(&self, task_id: &str) {
        let pids = {
            let mut tasks = self.tasks.lock().await;
            if let Some(info) = tasks.get_mut(task_id) {
                std::mem::take(&mut info.child_pids)
            } else {
                Vec::new()
            }
        };

        for pid in pids {
            Self::kill_pid(pid);
        }
    }

    pub async fn set_abort_handle(&self, task_id: &str, handle: AbortHandle) {
        let mut tasks = self.tasks.lock().await;
        if let Some(info) = tasks.get_mut(task_id) {
            if info.is_cancelled {
                handle.abort();
            } else {
                info.abort_handle = Some(handle);
            }
        }
    }

    pub async fn set_format(&self, task_id: &str, format: Option<String>) {
        if let Some(info) = self.tasks.lock().await.get_mut(task_id) {
            info.format = format;
        }
    }

    pub async fn format(&self, task_id: &str) -> Option<String> {
        self.tasks.lock().await.get(task_id).and_then(|t| t.format.clone())
    }

    pub async fn set_temp_dir(&self, task_id: &str, temp_dir: PathBuf) {
        let mut tasks = self.tasks.lock().await;
        if let Some(info) = tasks.get_mut(task_id) {
            info.temp_dir = Some(temp_dir);
        }
    }

    pub async fn set_target_dir(&self, task_id: &str, target_dir: PathBuf, root_dir: PathBuf) {
        let mut tasks = self.tasks.lock().await;
        if let Some(info) = tasks.get_mut(task_id) {
            info.target_dir = Some(target_dir);
            info.root_dir = Some(root_dir);
        }
    }

    pub async fn update_metadata(
        &self,
        task_id: &str,
        title: &str,
        artist: &str,
        cover_url: Option<&str>,
        tracks: Vec<(String, Option<i32>)>,
    ) {
        let mut tasks = self.tasks.lock().await;
        if let Some(info) = tasks.get_mut(task_id) {
            if !title.is_empty() {
                info.state.album_title = title.to_string();
            }
            if !artist.is_empty() {
                info.state.artist_name = artist.to_string();
            }
            if let Some(c) = cover_url {
                info.state.cover_url = Some(c.to_string());
            }
            if !tracks.is_empty() {
                info.state.total = tracks.len();
                info.state.tracks = tracks
                    .into_iter()
                    .enumerate()
                    .map(|(idx, (t_title, t_num))| TrackProgressState {
                        title: t_title,
                        track_number: t_num.or(Some((idx + 1) as i32)),
                        percent: 0,
                        status: "pending".to_string(),
                    })
                    .collect();
            }
        }
    }

    pub async fn update_track_progress(
        &self,
        task_id: &str,
        track_index: usize,
        track_title: &str,
        percent: i32,
    ) {
        let mut tasks = self.tasks.lock().await;
        if let Some(info) = tasks.get_mut(task_id) {
            while info.state.tracks.len() <= track_index {
                let idx = info.state.tracks.len();
                info.state.tracks.push(TrackProgressState {
                    title: format!("Track {}", idx + 1),
                    track_number: Some((idx + 1) as i32),
                    percent: 0,
                    status: "pending".to_string(),
                });
            }
            info.state.tracks[track_index].percent = percent.clamp(0, 100);
            if !track_title.is_empty() {
                info.state.tracks[track_index].title = track_title.to_string();
            }
            info.state.tracks[track_index].status = if percent >= 100 {
                "completed".to_string()
            } else {
                "downloading".to_string()
            };

            let count = info.state.tracks.len();
            if count > 0 {
                let sum: i32 = info.state.tracks.iter().map(|t| t.percent).sum();
                info.state.overall_percent = sum / (count as i32);
            } else {
                info.state.overall_percent = percent;
            }
        }
    }

    pub async fn update_task_progress(
        &self,
        task_id: &str,
        current: usize,
        total: usize,
        message: &str,
    ) {
        let mut tasks = self.tasks.lock().await;
        if let Some(info) = tasks.get_mut(task_id) {
            info.state.current = current;
            if total > 0 {
                info.state.total = total;
            }
            if !message.is_empty() {
                info.state.message = message.to_string();
            }
        }
    }

    pub async fn remove_task(&self, task_id: &str) -> Option<ActiveTaskInfo> {
        let mut tasks = self.tasks.lock().await;
        tasks.remove(task_id)
    }

    pub async fn get_all_states(&self) -> Vec<ActiveDownloadState> {
        let tasks = self.tasks.lock().await;
        tasks.values().map(|t| t.state.clone()).collect()
    }
}

pub struct DownloadQueue {
    pub tx: mpsc::UnboundedSender<DownloadTask>,
}

pub fn init_queue_worker(app: AppHandle) -> mpsc::UnboundedSender<DownloadTask> {
    let (tx, mut rx) = mpsc::unbounded_channel::<DownloadTask>();

    tauri::async_runtime::spawn(async move {
        println!("[Queue Worker] Core pipeline active. Awaiting jobs...");

        while let Some(task) = rx.recv().await {
            let task_id = task.id.clone();
            let app_clone = app.clone();

            let join_handle = tokio::spawn(async move {
                if let Err(e) = handle_download_task(task, app_clone).await {
                    eprintln!("[Queue Worker] Task failed: {e}");
                }
            });

            if let Some(registry) = app.try_state::<TaskRegistry>() {
                registry
                    .set_abort_handle(&task_id, join_handle.abort_handle())
                    .await;
            }

            let _ = join_handle.await;

            if let Some(registry) = app.try_state::<TaskRegistry>() {
                registry.remove_task(&task_id).await;
            }
        }
    });

    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    // Far above any real PID range, so kill_pid targets no live process.
    const FAKE_PID: u32 = 4_000_000_000;
    const FAKE_PID_AFTER_CANCEL: u32 = 4_000_000_001;

    fn make_state(id: &str) -> ActiveDownloadState {
        ActiveDownloadState {
            id: id.to_string(),
            platform: "bandcamp".to_string(),
            download_type: "album".to_string(),
            url: "https://example.com/album".to_string(),
            artist_name: String::new(),
            album_title: String::new(),
            cover_url: None,
            status: "queued".to_string(),
            current: 0,
            total: 0,
            overall_percent: 0,
            message: String::new(),
            tracks: Vec::new(),
        }
    }

    #[tokio::test]
    async fn mark_cancelled_sets_flag_and_clears_child_pids() {
        let registry = TaskRegistry::default();
        registry.register_task(make_state("task-1")).await;
        registry.register_pid("task-1", FAKE_PID).await;

        let pids_before = registry.tasks.lock().await.get("task-1").unwrap().child_pids.clone();
        assert_eq!(pids_before, vec![FAKE_PID]);

        registry.mark_cancelled("task-1").await;

        assert!(registry.is_cancelled("task-1").await);
        let tasks = registry.tasks.lock().await;
        assert!(tasks.get("task-1").unwrap().child_pids.is_empty());
    }

    #[tokio::test]
    async fn mark_cancelled_aborts_registered_future() {
        let registry = TaskRegistry::default();
        registry.register_task(make_state("task-2")).await;

        let join = tokio::spawn(async {
            std::future::pending::<()>().await;
        });
        registry
            .set_abort_handle("task-2", join.abort_handle())
            .await;

        registry.mark_cancelled("task-2").await;

        let err = join
            .await
            .expect_err("aborted task should not complete normally");
        assert!(err.is_cancelled());
    }

    #[tokio::test]
    async fn set_temp_dir_stores_path() {
        let registry = TaskRegistry::default();
        registry.register_task(make_state("task-3")).await;

        let temp_dir = PathBuf::from("some").join("temp").join("task-3");
        registry.set_temp_dir("task-3", temp_dir.clone()).await;

        let tasks = registry.tasks.lock().await;
        assert_eq!(tasks.get("task-3").unwrap().temp_dir, Some(temp_dir));
    }

    #[tokio::test]
    async fn register_pid_after_cancel_is_not_tracked() {
        let registry = TaskRegistry::default();
        registry.register_task(make_state("task-4")).await;
        registry.mark_cancelled("task-4").await;

        registry
            .register_pid("task-4", FAKE_PID_AFTER_CANCEL)
            .await;

        let tasks = registry.tasks.lock().await;
        assert!(tasks.get("task-4").unwrap().child_pids.is_empty());
    }

    #[tokio::test]
    async fn format_is_none_until_one_is_set() {
        let registry = TaskRegistry::default();
        registry.register_task(make_state("task-5")).await;
        assert_eq!(registry.format("task-5").await, None);
    }

    #[tokio::test]
    async fn set_format_is_kept_per_task() {
        let registry = TaskRegistry::default();
        registry.register_task(make_state("task-6")).await;
        registry.register_task(make_state("task-7")).await;

        registry.set_format("task-6", Some("251".to_string())).await;

        assert_eq!(registry.format("task-6").await.as_deref(), Some("251"));
        assert_eq!(registry.format("task-7").await, None);
    }

    #[tokio::test]
    async fn set_format_replaces_and_clears_the_choice() {
        let registry = TaskRegistry::default();
        registry.register_task(make_state("task-8")).await;

        registry.set_format("task-8", Some("251".to_string())).await;
        registry.set_format("task-8", Some("mp3-320".to_string())).await;
        assert_eq!(registry.format("task-8").await.as_deref(), Some("mp3-320"));

        registry.set_format("task-8", None).await;
        assert_eq!(registry.format("task-8").await, None);
    }

    #[tokio::test]
    async fn unknown_task_has_no_format_and_setting_one_is_a_no_op() {
        let registry = TaskRegistry::default();
        registry.set_format("missing", Some("251".to_string())).await;

        assert_eq!(registry.format("missing").await, None);
        assert!(registry.tasks.lock().await.is_empty());
    }
}
