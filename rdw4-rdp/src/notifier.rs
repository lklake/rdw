use std::{os::unix::prelude::*, sync::Arc};

use freerdp::{
    winpr::{FdMode, Handle},
    RdpError, Result,
};
use rdw::gtk::{
    gio::{self, NONE_CANCELLABLE},
    glib,
    prelude::*,
};

#[derive(Debug)]
struct NotifierInner {
    fd: RawFd,
}

impl Drop for NotifierInner {
    fn drop(&mut self) {
        let _ = nix::unistd::close(self.fd);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Notifier {
    inner: Arc<NotifierInner>,
}

impl Notifier {
    pub(crate) fn new() -> Result<Self> {
        // TODO: non-Linux
        use nix::sys::eventfd::*;
        let fd = eventfd(
            0,
            EfdFlags::EFD_CLOEXEC | EfdFlags::EFD_NONBLOCK | EfdFlags::EFD_SEMAPHORE,
        )
        .map_err(|e| RdpError::Failed(format!("eventfd() failed: {}", e)))?;

        Ok(Self {
            inner: Arc::new(NotifierInner { fd }),
        })
    }

    pub(crate) fn handle(&self) -> Handle {
        Handle::new_fd_event(&[], false, false, self.inner.fd, FdMode::READ)
    }

    pub(crate) async fn notify(&self) -> Result<()> {
        let st = unsafe { gio::UnixOutputStream::with_fd(self.inner.fd) };
        let buffer = 1u64.to_ne_bytes();
        st.write_all_async_future(buffer, glib::Priority::default())
            .await
            .map_err(|_| RdpError::Failed("notify() failed".into()))?;
        Ok(())
    }

    pub(crate) fn read_sync(&self) -> Result<()> {
        let st = unsafe { gio::UnixInputStream::with_fd(self.inner.fd) };
        let buffer = 1u64.to_ne_bytes();
        st.read_all(buffer, NONE_CANCELLABLE)
            .map_err(|e| RdpError::Failed(format!("read() failed: {}", e)))?;
        Ok(())
    }
}
