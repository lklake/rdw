use freerdp::{sys, RdpErr};
use std::os::raw::c_void;

use rdw::gtk::{
    self, gio,
    glib::{self, translate::*},
    prelude::*,
};

use crate::display::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, glib::ErrorDomain)]
#[error_domain(name = "RdwRdpError")]
pub enum Error {
    Failed,
}

#[no_mangle]
pub extern "C" fn rdw_rdp_display_get_type() -> glib::ffi::GType {
    gtk::init().unwrap();
    <Display as glib::types::StaticType>::static_type().into_glib()
}

/// rdw_rdp_display_connect_async:
/// @dpy: A #RdwDisplay
/// @cancellable: (nullable): optional #GCancellable object, %NULL to ignore
/// @callback: (scope async): a #GAsyncReadyCallback to call when the request is satisfied
/// @user_data: (closure): the data to pass to callback function
#[no_mangle]
pub unsafe extern "C" fn rdw_rdp_display_connect_async(
    dpy: *mut RdwRdpDisplay,
    _cancellable: *mut gio::ffi::GCancellable,
    callback: gio::ffi::GAsyncReadyCallback,
    user_data: *mut c_void,
) {
    let this_ptr = dpy as *mut _;
    let this: Display = from_glib_none(dpy);
    let callback = callback.unwrap();

    let closure = move |task: gio::LocalTask<bool>, _: Option<&Display>| {
        let result: *mut gio::ffi::GAsyncResult =
            task.upcast_ref::<gio::AsyncResult>().to_glib_none().0;
        callback(this_ptr, result, user_data)
    };

    let task = gio::LocalTask::new(Some(&this), gio::Cancellable::NONE, closure);

    glib::MainContext::default().spawn_local(async move {
        let res = this
            .rdp_connect()
            .await
            .map_err(|_| glib::Error::new(Error::Failed, "Connect failed"))
            .map(|_| true);
        task.return_result(res);
    });
}

/// rdw_rdp_display_connect_finish:
/// @dpy: A #RdwDisplay
/// @res: a #GAsyncResult
/// @error: a #GError
#[no_mangle]
pub unsafe extern "C" fn rdw_rdp_display_connect_finish(
    _dpy: *mut RdwRdpDisplay,
    res: *mut gio::ffi::GAsyncResult,
    error: *mut *mut glib::ffi::GError,
) -> bool {
    let task = gio::Task::<bool>::from_glib_none(res as *mut gio::ffi::GTask);

    match task.propagate() {
        Ok(_) => true,
        Err(e) => {
            if !error.is_null() {
                *error = e.into_glib_ptr();
            }
            false
        }
    }
}

/// rdw_rdp_display_disconnect_async:
/// @dpy: A #RdwDisplay
/// @cancellable: (nullable): optional #GCancellable object, %NULL to ignore
/// @callback: (scope async): a #GAsyncReadyCallback to call when the request is satisfied
/// @user_data: (closure): the data to pass to callback function
#[no_mangle]
pub unsafe extern "C" fn rdw_rdp_display_disconnect_async(
    dpy: *mut RdwRdpDisplay,
    _cancellable: *mut gio::ffi::GCancellable,
    callback: gio::ffi::GAsyncReadyCallback,
    user_data: *mut c_void,
) {
    let this_ptr = dpy as *mut _;
    let this: Display = from_glib_none(dpy);
    let callback = callback.unwrap();

    let closure = move |task: gio::LocalTask<bool>, _: Option<&Display>| {
        let result: *mut gio::ffi::GAsyncResult =
            task.upcast_ref::<gio::AsyncResult>().to_glib_none().0;
        callback(this_ptr, result, user_data)
    };

    let task = gio::LocalTask::new(Some(&this), gio::Cancellable::NONE, closure);

    glib::MainContext::default().spawn_local(async move {
        let res = this
            .rdp_disconnect()
            .await
            .map_err(|_| glib::Error::new(Error::Failed, "Disconnect failed"))
            .map(|_| true);
        task.return_result(res);
    });
}

/// rdw_rdp_display_disconnect_finish:
/// @dpy: A #RdwDisplay
/// @res: a #GAsyncResult
/// @error: a #GError
#[no_mangle]
pub unsafe extern "C" fn rdw_rdp_display_disconnect_finish(
    _dpy: *mut RdwRdpDisplay,
    res: *mut gio::ffi::GAsyncResult,
    error: *mut *mut glib::ffi::GError,
) -> bool {
    let task = gio::Task::<bool>::from_glib_none(res as *mut gio::ffi::GTask);

    match task.propagate() {
        Ok(_) => true,
        Err(e) => {
            if !error.is_null() {
                *error = e.into_glib_ptr();
            }
            false
        }
    }
}

/// rdw_rdp_display_get_settings:
/// @dpy: A #RdwDisplay
///
/// Returns: (transfer none): the associated FreeRDP settings
#[no_mangle]
pub unsafe extern "C" fn rdw_rdp_display_get_settings(
    dpy: *mut RdwRdpDisplay,
) -> *mut sys::rdpSettings {
    let this: &Display = &from_glib_borrow(dpy);
    let mut settings = std::ptr::null_mut();
    this.with_settings(|s| {
        settings = s.as_ptr();
        Ok(())
    })
    .unwrap();
    settings
}

/// rdw_rdp_display_get_last_error:
/// @dpy: A #RdwDisplay
///
/// Returns: the last FreeRDP error
#[no_mangle]
pub unsafe extern "C" fn rdw_rdp_display_get_last_error(dpy: *mut RdwRdpDisplay) -> u32 {
    let this: &Display = &from_glib_borrow(dpy);
    let Some(err) = this.last_error() else {
        return 0;
    };
    match err {
        RdpErr::RdpErrBase(b) => b as _,
        RdpErr::RdpErrInfo(i) => i as _,
        RdpErr::RdpErrConnect(c) => c as _,
    }
}
