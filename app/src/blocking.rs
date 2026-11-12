//! Serialize a resource's blocking work without parking an async runtime worker.
//! The permit is acquired before spawning, so queued IPC cannot fill the
//! blocking pool with threads merely waiting for the same session mutex.
use tagrex_commands::ErrorDto;

pub struct Queue(tauri::async_runtime::Mutex<()>);

impl Queue {
    pub const fn new() -> Self {
        Self(tauri::async_runtime::Mutex::const_new(()))
    }

    pub async fn run<T: Send + 'static>(
        &self,
        job: impl FnOnce() -> Result<T, ErrorDto> + Send + 'static,
    ) -> Result<T, ErrorDto> {
        let _permit = self.0.lock().await;
        tauri::async_runtime::spawn_blocking(job)
            .await
            .map_err(|error| ErrorDto::plain(format!("Background task failed: {error}")))?
    }
}

#[cfg(test)]
mod tests {
    use super::Queue;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn jobs_leave_the_calling_thread() {
        let caller = std::thread::current().id();
        let worker =
            tauri::async_runtime::block_on(Queue::new().run(|| Ok(std::thread::current().id())))
                .unwrap();
        assert_ne!(caller, worker);
    }

    #[test]
    fn a_busy_resource_does_not_block_an_unrelated_queue() {
        let busy = Arc::new(Queue::new());
        let (started, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        let first = tauri::async_runtime::spawn({
            let busy = busy.clone();
            async move {
                busy.run(move || {
                    started.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok(())
                })
                .await
            }
        });
        ready.recv_timeout(Duration::from_secs(5)).unwrap();
        let (finished, observed) = mpsc::channel();
        let independent = tauri::async_runtime::spawn(async move {
            Queue::new()
                .run(move || {
                    finished.send(()).unwrap();
                    Ok(())
                })
                .await
        });
        let responsive = observed.recv_timeout(Duration::from_secs(2)).is_ok();
        release.send(()).unwrap();
        tauri::async_runtime::block_on(first).unwrap().unwrap();
        tauri::async_runtime::block_on(independent)
            .unwrap()
            .unwrap();
        assert!(responsive, "unrelated work waited for the busy resource");
    }
}
