#[cfg(feature = "driver")]
mod driver_only {
    use kcom::{spawn_task, KernelTimerFuture, NTSTATUS, STATUS_SUCCESS, TaskTracker};

    pub fn run() -> NTSTATUS {
        let fut = async move {
            let timer = match KernelTimerFuture::new(-10_000) {
                Ok(timer) => timer,
                Err(status) => return status,
            };
            timer.await;
            STATUS_SUCCESS
        };

        let tracker = TaskTracker::new();
        let status = spawn_task(&tracker, fut);
        tracker.drain();
        status
    }
}

fn main() {
    #[cfg(feature = "driver")]
    {
        let status = driver_only::run();
        let _ = status;
    }

    #[cfg(not(feature = "driver"))]
    {
    }
}
