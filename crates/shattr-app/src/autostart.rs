//! Launch at login, through the per-user Run key. Only changed when the user
//! ticks the tray menu item.

#[cfg(windows)]
mod imp {
    use anyhow::Result;
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RRF_RT_REG_SZ, RegCloseKey, RegDeleteValueW, RegGetValueW, RegOpenKeyExW,
        RegSetValueExW,
    };
    use windows::core::{HSTRING, w};

    const RUN: windows::core::PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
    const NAME: windows::core::PCWSTR = w!("Shattr");

    pub fn is_enabled() -> bool {
        let mut size = 0u32;
        unsafe { RegGetValueW(HKEY_CURRENT_USER, RUN, NAME, RRF_RT_REG_SZ, None, None, Some(&mut size)).is_ok() }
    }

    pub fn set(enabled: bool) -> Result<()> {
        unsafe {
            let mut key = HKEY::default();
            RegOpenKeyExW(HKEY_CURRENT_USER, RUN, None, KEY_SET_VALUE, &mut key).ok()?;
            let result = if enabled {
                let exe = std::env::current_exe()?;
                let value = HSTRING::from(format!("\"{}\"", exe.display()));
                let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, (value.len() + 1) * 2);
                RegSetValueExW(key, NAME, None, REG_SZ, Some(bytes)).ok()
            } else {
                let r = RegDeleteValueW(key, NAME);
                if r.0 == 2 { Ok(()) } else { r.ok() }
            };
            let _ = RegCloseKey(key);
            Ok(result?)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn is_enabled() -> bool {
        false
    }

    pub fn set(_: bool) -> anyhow::Result<()> {
        anyhow::bail!("start at login is not available on this platform yet")
    }
}

pub use imp::{is_enabled, set};
