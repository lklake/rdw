#[cfg(not(feature = "bindings"))]
mod imp {
    use super::*;
    use windows::core::Result;
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoA, SPI_GETMOUSE, SPI_GETMOUSESPEED, SPI_SETMOUSE, SPI_SETMOUSESPEED,
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
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
}

#[cfg(not(feature = "bindings"))]
pub(crate) use imp::*;
