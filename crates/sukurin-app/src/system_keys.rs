//! Keys Windows claims for itself. On Windows 11 PrtSc opens Snipping Tool
//! unless the user switches that off, and while it does, the shell takes the
//! key ahead of any registered hotkey. Registering bare PrtSc still succeeds,
//! so the registry value is the only signal.

pub use imp::{open_keyboard_settings, release_prtsc, snipping_owns_prtsc};

/// Shown after a release, since it's unconfirmed whether the shell lets go of
/// PrtSc before the next sign-in.
pub const SIGN_OUT_HINT: &str = "If PrtSc still opens Snipping Tool, sign out of Windows and back in.";

#[cfg(windows)]
mod imp {
    use anyhow::Result;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegGetValueW, RegSetValueExW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_SETTINGCHANGE};
    use windows::core::{PCWSTR, w};

    const KEYBOARD: PCWSTR = w!("Control Panel\\Keyboard");
    const SNIPPING: PCWSTR = w!("PrintScreenKeyForSnippingEnabled");

    pub fn snipping_owns_prtsc() -> bool {
        snipping_on(read_dword(KEYBOARD, SNIPPING), windows_build())
    }

    /// Windows 11 22H2 (build 22621) turned the setting on for anyone who
    /// never touched it; earlier builds left it off.
    pub(super) fn snipping_on(value: Option<u32>, build: u32) -> bool {
        match value {
            Some(v) => v == 1,
            None => build >= 22621,
        }
    }

    /// Unreadable counts as a current build.
    pub(super) fn windows_build() -> u32 {
        let mut buf = [0u16; 16];
        let mut n = size_of_val(&buf) as u32;
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                w!("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion"),
                w!("CurrentBuildNumber"),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut n),
            )
        };
        if r.is_err() {
            return u32::MAX;
        }
        let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..len]).parse().unwrap_or(u32::MAX)
    }

    pub fn release_prtsc() -> Result<()> {
        write_dword(KEYBOARD, SNIPPING, 0)?;
        // The same notification the Settings toggle sends after writing the value.
        unsafe {
            SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(w!("Keyboard").as_ptr() as isize),
                SMTO_ABORTIFHUNG,
                200,
                None,
            );
        }
        Ok(())
    }

    pub fn open_keyboard_settings() {
        if let Err(e) = std::process::Command::new("explorer").arg("ms-settings:easeofaccess-keyboard").spawn() {
            log::error!("could not open keyboard settings: {e}");
        }
    }

    pub(super) fn read_dword(subkey: PCWSTR, name: PCWSTR) -> Option<u32> {
        let mut v = 0u32;
        let mut n = 4u32;
        let r = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, subkey, name, RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut n))
        };
        r.is_ok().then_some(v)
    }

    pub(super) fn write_dword(subkey: PCWSTR, name: PCWSTR, value: u32) -> Result<()> {
        unsafe {
            let mut key = HKEY::default();
            RegCreateKeyExW(HKEY_CURRENT_USER, subkey, None, PCWSTR::null(), REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None, &mut key, None).ok()?;
            let r = RegSetValueExW(key, name, None, REG_DWORD, Some(&value.to_le_bytes()));
            let _ = RegCloseKey(key);
            Ok(r.ok()?)
        }
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn snipping_owns_prtsc() -> bool {
        false
    }

    pub fn release_prtsc() -> anyhow::Result<()> {
        anyhow::bail!("only Windows reserves PrtSc")
    }

    pub fn open_keyboard_settings() {}
}

#[cfg(all(test, windows))]
mod tests {
    use super::imp::{read_dword, snipping_on, windows_build, write_dword};
    use windows::Win32::System::Registry::{HKEY, HKEY_CURRENT_USER, KEY_READ, RegCloseKey, RegDeleteKeyValueW, RegDeleteKeyW, RegOpenKeyExW};
    use windows::core::{PCWSTR, w};

    const PARENT: PCWSTR = w!("Software\\Sukurin");
    const SCRATCH: PCWSTR = w!("Software\\Sukurin\\Test");
    const NAME: PCWSTR = w!("Value");

    fn exists(subkey: PCWSTR) -> bool {
        let mut key = HKEY::default();
        let ok = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, subkey, None, KEY_READ, &mut key) }.is_ok();
        if ok {
            let _ = unsafe { RegCloseKey(key) };
        }
        ok
    }

    /// Removes the scratch key, and its parent if the test created it, even
    /// when an assertion fails.
    struct Cleanup {
        parent_existed: bool,
    }

    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                let _ = RegDeleteKeyW(HKEY_CURRENT_USER, SCRATCH);
                if !self.parent_existed {
                    let _ = RegDeleteKeyW(HKEY_CURRENT_USER, PARENT);
                }
            }
        }
    }

    #[test]
    fn snipping_tool_owns_prtsc_when_set_or_left_at_a_new_builds_default() {
        assert!(snipping_on(Some(1), 19045));
        assert!(!snipping_on(Some(0), 26200));
        assert!(snipping_on(None, 22621));
        assert!(snipping_on(None, u32::MAX));
        assert!(!snipping_on(None, 22000));
        assert!(!snipping_on(None, 19045));
    }

    #[test]
    fn reads_the_real_build_number() {
        let build = windows_build();
        assert!((10240..u32::MAX).contains(&build), "{build}");
    }

    #[test]
    fn dword_reads_writes_and_deletes_in_a_scratch_key() {
        let cleanup = Cleanup { parent_existed: exists(PARENT) };
        assert_eq!(read_dword(SCRATCH, NAME), None);
        write_dword(SCRATCH, NAME, 0).unwrap();
        assert_eq!(read_dword(SCRATCH, NAME), Some(0));
        write_dword(SCRATCH, NAME, 7).unwrap();
        assert_eq!(read_dword(SCRATCH, NAME), Some(7));
        unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, SCRATCH, NAME) }.ok().unwrap();
        assert_eq!(read_dword(SCRATCH, NAME), None);
        drop(cleanup);
        assert!(!exists(SCRATCH));
    }
}
