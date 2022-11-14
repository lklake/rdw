#[cfg(not(feature = "bindings"))]
mod imp {
    use gtk::prelude::*;
    use windows::core::Result;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowsHookExA, SystemParametersInfoA, UnhookWindowsHookEx, HHOOK, SPI_GETMOUSE,
        SPI_GETMOUSESPEED, SPI_SETMOUSE, SPI_SETMOUSESPEED, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };

    pub(crate) fn spi_set_mouse(mut mouse: [isize; 3]) -> Result<()> {
        unsafe {
            SystemParametersInfoA(
                SPI_SETMOUSE,
                0,
                Some(mouse.as_mut_ptr() as _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .ok()
        }
    }

    pub(crate) fn spi_set_mouse_speed(mut speed: isize) -> Result<()> {
        unsafe {
            SystemParametersInfoA(
                SPI_SETMOUSESPEED,
                0,
                Some(&mut speed as *mut _ as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .ok()
        }
    }

    pub(crate) fn spi_get_mouse() -> Result<[isize; 3]> {
        let mut mouse: [isize; 3] = Default::default();

        unsafe {
            SystemParametersInfoA(
                SPI_GETMOUSE,
                0,
                Some(mouse.as_mut_ptr() as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .ok()?
        }

        Ok(mouse)
    }

    pub(crate) fn spi_get_mouse_speed() -> Result<isize> {
        let mut speed: isize = Default::default();

        unsafe {
            SystemParametersInfoA(
                SPI_GETMOUSESPEED,
                0,
                Some(&mut speed as *mut _ as *mut _),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .ok()?
        }

        Ok(speed)
    }

    pub(crate) fn hook_keyboard() -> Result<HHOOK> {
        use windows::Win32::System::LibraryLoader::GetModuleHandleA;
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        use windows::Win32::UI::WindowsAndMessaging::{
            CallNextHookEx, GetForegroundWindow, SendMessageA, HC_ACTION, KBDLLHOOKSTRUCT,
            WH_KEYBOARD_LL, WM_KEYUP,
        };

        // code adapted from spice-gtk, seems to be doing ok..
        unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            if code != HC_ACTION as i32 || wparam.0 == WM_KEYUP as usize {
                return CallNextHookEx(None, code, wparam, lparam);
            }

            let top = gtk::Window::list_toplevels();
            if !top
                .iter()
                .find(|&w| w.clone().downcast::<gtk::Window>().unwrap().is_active())
                .is_some()
            {
                return CallNextHookEx(None, code, wparam, lparam);
            };
            let h = GetForegroundWindow();
            let kb = &*std::mem::transmute::<_, &mut KBDLLHOOKSTRUCT>(lparam);
            let mut dwmsg = kb.flags.0 << 24 | kb.scanCode << 16 | 1;

            match VIRTUAL_KEY(kb.vkCode as _) {
                VK_NUMLOCK | VK_RSHIFT => {
                    dwmsg &= !(1 << 24);
                    SendMessageA(h, wparam.0 as _, WPARAM(kb.vkCode as _), LPARAM(dwmsg as _));
                }
                VK_CAPITAL | VK_SCROLL | VK_LSHIFT | VK_LMENU | VK_RMENU => (),
                VK_LCONTROL => {
                    // When pressing AltGr, an extra VK_LCONTROL with a special
                    // scancode with bit 9 set is sent. Let's ignore the extra
                    // VK_LCONTROL, as that will make AltGr misbehave.
                    if kb.scanCode & 0x200 != 0 {
                        return LRESULT(1);
                    }
                }
                _ => {
                    SendMessageA(h, wparam.0 as _, WPARAM(kb.vkCode as _), LPARAM(dwmsg as _));
                    return LRESULT(1);
                }
            }

            CallNextHookEx(None, code, wparam, lparam)
        }

        unsafe { SetWindowsHookExA(WH_KEYBOARD_LL, Some(hook), GetModuleHandleA(None)?, 0) }
    }

    pub(crate) fn unhook(hook: HHOOK) -> Result<()> {
        unsafe { UnhookWindowsHookEx(hook).ok() }
    }

    pub(crate) fn hook_mouse() -> Result<HHOOK> {
        use windows::Win32::System::LibraryLoader::GetModuleHandleA;
        use windows::Win32::UI::WindowsAndMessaging::{
            CallNextHookEx, HC_ACTION, WH_MOUSE_LL, WM_MOUSEMOVE,
        };

        unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
            if code != HC_ACTION as i32 {
                return CallNextHookEx(None, code, wparam, lparam);
            }
            if wparam.0 == WM_MOUSEMOVE as _ {
                return LRESULT(1);
            }

            CallNextHookEx(None, code, wparam, lparam)
        }

        unsafe { SetWindowsHookExA(WH_MOUSE_LL, Some(hook), GetModuleHandleA(None)?, 0) }
    }
}

#[cfg(not(feature = "bindings"))]
pub(crate) use imp::*;
