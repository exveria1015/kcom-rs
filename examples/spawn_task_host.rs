use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

use kcom::{spawn_task, NTSTATUS, STATUS_SUCCESS};

struct YieldOnce {
    yielded: bool,
}

impl Future for YieldOnce {
    type Output = NTSTATUS;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.yielded {
            Poll::Ready(STATUS_SUCCESS)
        } else {
            self.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
}

fn main() {
    let status = spawn_task(YieldOnce { yielded: false });
    assert_eq!(status, STATUS_SUCCESS);
}
