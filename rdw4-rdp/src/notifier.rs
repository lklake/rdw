use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::prelude::*;

#[cfg(windows)]
use windows::{
    core::PCSTR,
    Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::Threading::{CreateEventA, ResetEvent, SetEvent},
    },
};

use freerdp::{winpr::Handle, RdpError, Result};

#[cfg(unix)]
use rdw::gtk::{
    gio::{self, Cancellable},
    glib,
    prelude::*,
};

#[derive(Debug)]
struct NotifierInner {
    #[cfg(windows)]
    event: HANDLE,
    #[cfg(unix)]
    fd: RawFd,
}

impl Drop for NotifierInner {
    fn drop(&mut self) {
        #[cfg(windows)]
        unsafe {
            CloseHandle(self.event).unwrap();
        }
        #[cfg(unix)]
        let _ = nix::unistd::close(self.fd);
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Notifier {
    inner: Arc<NotifierInner>,
}

impl Notifier {
    pub(crate) fn new() -> Result<Self> {
        #[cfg(windows)]
        {
            let event = unsafe {
                CreateEventA(None, true, false, PCSTR::null())
                    .map_err(|e| RdpError::Failed(format!("CreateEvent failed: {}", e)))?
            };
            Ok(Self {
                inner: Arc::new(NotifierInner { event }),
            })
        }
        #[cfg(unix)]
        {
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
    }

    pub(crate) fn handle(&self) -> Result<Handle> {
        #[cfg(windows)]
        {
            Ok(Handle::new_from_raw(self.inner.event.0 as _, false))
        }
        #[cfg(unix)]
        {
            Ok(Handle::new_fd_event(
                &[],
                false,
                false,
                nix::unistd::dup(self.inner.fd)
                    .map_err(|e| RdpError::Failed(format!("dup() failed: {}", e)))?,
                freerdp::winpr::FdMode::READ,
            ))
        }
    }

    pub(crate) async fn notify(&self) -> Result<()> {
        #[cfg(windows)]
        {
            unsafe {
                SetEvent(self.inner.event)
                    .ok()
                    .map_err(|e| RdpError::Failed(format!("SetEvent failed: {}", e)))
            }
        }
        #[cfg(unix)]
        {
            let st = unsafe { gio::UnixOutputStream::with_fd(self.inner.fd) };
            let buffer = 1u64.to_ne_bytes();
            st.write_all_future(buffer, glib::Priority::default())
                .await
                .map_err(|_| RdpError::Failed("notify() failed".into()))?;
            Ok(())
        }
    }

    pub(crate) fn read_sync(&self) -> Result<()> {
        #[cfg(windows)]
        {
            unsafe {
                ResetEvent(self.inner.event)
                    .ok()
                    .map_err(|e| RdpError::Failed(format!("ResetEvent failed: {}", e)))
            }
        }
        #[cfg(unix)]
        {
            let st = unsafe { gio::UnixInputStream::with_fd(self.inner.fd) };
            let buffer = 1u64.to_ne_bytes();
            st.read_all(buffer, Cancellable::NONE)
                .map_err(|e| RdpError::Failed(format!("read() failed: {}", e)))?;
            Ok(())
        }
    }
}
