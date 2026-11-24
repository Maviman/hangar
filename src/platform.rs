//! OS integration: launching, icons, version info, cursor position.
//! Windows uses Win32 directly (windows-sys). Other OSes get dev fallbacks.

use std::path::Path;

pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

#[cfg(windows)]
#[allow(unsafe_op_in_unsafe_fn)]
mod imp {
    use super::Rgba;
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;
    use std::ptr::{null, null_mut};
    use windows_sys::core::GUID;
    use windows_sys::Win32::Foundation::POINT;
    use windows_sys::Win32::Graphics::Gdi::*;
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::Environment::ExpandEnvironmentStringsW;
    use windows_sys::Win32::UI::Controls::{ImageList_GetIcon, HIMAGELIST, ILD_TRANSPARENT};
    use windows_sys::Win32::UI::Shell::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::*;

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }
    fn wstr(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn exe_description(path: &Path) -> Option<String> {
        unsafe {
            let wp = wide(path.as_os_str());
            let mut handle = 0u32;
            let size = GetFileVersionInfoSizeW(wp.as_ptr(), &mut handle);
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            if GetFileVersionInfoW(wp.as_ptr(), 0, size, buf.as_mut_ptr() as *mut c_void) == 0 {
                return None;
            }
            // Candidate language/codepage pairs: declared ones first, then common defaults.
            let mut langs: Vec<(u16, u16)> = Vec::new();
            let mut p: *mut c_void = null_mut();
            let mut len = 0u32;
            let q = wstr("\\VarFileInfo\\Translation");
            if VerQueryValueW(buf.as_ptr() as *const c_void, q.as_ptr(), &mut p, &mut len) != 0 && !p.is_null() {
                let n = len as usize / 4;
                let pairs = std::slice::from_raw_parts(p as *const u16, n * 2);
                for i in 0..n {
                    langs.push((pairs[i * 2], pairs[i * 2 + 1]));
                }
            }
            langs.extend([(0x0409, 0x04B0), (0x0409, 0x04E4), (0x0000, 0x04B0)]);
            for field in ["FileDescription", "ProductName"] {
                for (lang, cp) in &langs {
                    let q = wstr(&format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\{field}"));
                    let mut p: *mut c_void = null_mut();
                    let mut len = 0u32;
                    if VerQueryValueW(buf.as_ptr() as *const c_void, q.as_ptr(), &mut p, &mut len) != 0
                        && !p.is_null()
                        && len > 1
                    {
                        let s = std::slice::from_raw_parts(p as *const u16, len as usize);
                        let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                        let v = String::from_utf16_lossy(&s[..end]).trim().to_string();
                        if !v.is_empty() {
                            return Some(v);
                        }
                    }
                }
            }
            None
        }
    }

    pub fn expand_env(s: &str) -> String {
        if !s.contains('%') {
            return s.to_string();
        }
        unsafe {
            let src = wstr(s);
            let mut dst = vec![0u16; 1024];
            let n = ExpandEnvironmentStringsW(src.as_ptr(), dst.as_mut_ptr(), dst.len() as u32);
            if n == 0 || n as usize > dst.len() {
                return s.to_string();
            }
            String::from_utf16_lossy(&dst[..(n as usize).saturating_sub(1)])
        }
    }

    pub fn launch(path: &Path) -> Result<(), String> {
        unsafe {
            let file = wide(path.as_os_str());
            let op = wstr("open");
            let dir = path.parent().map(|d| wide(d.as_os_str()));
            let r = ShellExecuteW(
                null_mut(),
                op.as_ptr(),
                file.as_ptr(),
                null(),
                dir.as_ref().map_or(null(), |d| d.as_ptr()),
                SW_SHOWNORMAL,
            );
            if r as isize > 32 {
                Ok(())
            } else {
                Err(format!("ShellExecute error {}", r as isize))
            }
        }
    }

    pub fn reveal(path: &Path) -> Result<(), String> {
        unsafe {
            let exe = wstr("explorer.exe");
            let args = wstr(&format!("/select,\"{}\"", path.display()));
            let op = wstr("open");
            let r = ShellExecuteW(null_mut(), op.as_ptr(), exe.as_ptr(), args.as_ptr(), null(), SW_SHOWNORMAL);
            if r as isize > 32 { Ok(()) } else { Err("explorer failed".into()) }
        }
    }


    /// Native "Open" dialog to relocate a missing item. `keep_lnk` stops the
    /// dialog from resolving a picked .lnk to its target.
    pub fn pick_file(start: Option<&Path>, ext: &str, keep_lnk: bool) -> Option<std::path::PathBuf> {
        use windows_sys::Win32::UI::Controls::Dialogs::*;
        unsafe {
            let filter = if ext.is_empty() {
                "All files\0*.*\0\0".to_string()
            } else {
                format!("Same type (*.{ext})\0*.{ext}\0All files\0*.*\0\0")
            };
            let filter: Vec<u16> = filter.encode_utf16().collect();
            let mut buf = vec![0u16; 4096];
            let dir = start.map(|d| wide(d.as_os_str()));
            let title = wstr("HANGAR // RELOCATE");
            let mut ofn: OPENFILENAMEW = std::mem::zeroed();
            ofn.lStructSize = std::mem::size_of::<OPENFILENAMEW>() as u32;
            ofn.lpstrFilter = filter.as_ptr();
            ofn.nFilterIndex = 1;
            ofn.lpstrFile = buf.as_mut_ptr();
            ofn.nMaxFile = buf.len() as u32;
            ofn.lpstrInitialDir = dir.as_ref().map_or(null(), |d| d.as_ptr());
            ofn.lpstrTitle = title.as_ptr();
            ofn.Flags = OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR
                | if keep_lnk { OFN_NODEREFERENCELINKS } else { 0 };
            if GetOpenFileNameW(&mut ofn) == 0 {
                return None;
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..end])))
        }
    }

    /// Folder picker for relocating a docked folder.
    pub fn pick_folder() -> Option<std::path::PathBuf> {
        unsafe {
            let title = wstr("HANGAR // RELOCATE FOLDER");
            let mut bi: BROWSEINFOW = std::mem::zeroed();
            bi.lpszTitle = title.as_ptr();
            bi.ulFlags = BIF_RETURNONLYFSDIRS | BIF_NEWDIALOGSTYLE;
            let pidl = SHBrowseForFolderW(&bi);
            if pidl.is_null() {
                return None;
            }
            let mut buf = vec![0u16; 1024];
            let ok = SHGetPathFromIDListW(pidl, buf.as_mut_ptr()) != 0;
            windows_sys::Win32::System::Com::CoTaskMemFree(pidl as *const c_void);
            if !ok {
                return None;
            }
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            Some(std::path::PathBuf::from(String::from_utf16_lossy(&buf[..end])))
        }
    }

    pub const WINDOW_TITLE: &str = "HANGAR // LAUNCH INDEX";

    /// True if this is the only running copy. The mutex lives until the process exits.
    pub fn acquire_instance() -> bool {
        use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
        use windows_sys::Win32::System::Threading::CreateMutexW;
        unsafe {
            let name = wstr("Local\\HangarLauncher.SingleInstance");
            let h = CreateMutexW(null(), 0, name.as_ptr());
            !(h.is_null() || GetLastError() == ERROR_ALREADY_EXISTS)
        }
    }

    /// Bring the already-running window to the front.
    pub fn focus_existing() {
        unsafe {
            let title = wstr(WINDOW_TITLE);
            let h = FindWindowW(null(), title.as_ptr());
            if !h.is_null() {
                if IsIconic(h) != 0 {
                    ShowWindow(h, SW_RESTORE);
                }
                SetForegroundWindow(h);
            }
        }
    }

    pub fn message_box(title: &str, text: &str) {
        unsafe {
            let (t, m) = (wstr(title), wstr(text));
            MessageBoxW(null_mut(), m.as_ptr(), t.as_ptr(), MB_OK | MB_ICONERROR);
        }
    }

    /// Shell calls on a worker thread need COM initialised on that thread.
    pub fn thread_init() {
        unsafe {
            windows_sys::Win32::System::Com::CoInitializeEx(
                null(),
                windows_sys::Win32::System::Com::COINIT_APARTMENTTHREADED as u32,
            );
        }
    }

    /// Cursor in physical screen pixels (works during OLE drag-drop, unlike
    /// window mouse events).
    pub fn cursor_screen_pos() -> Option<(f32, f32)> {
        unsafe {
            let mut p = POINT { x: 0, y: 0 };
            if GetCursorPos(&mut p) != 0 { Some((p.x as f32, p.y as f32)) } else { None }
        }
    }

    const IID_IIMAGELIST: GUID = GUID {
        data1: 0x46EB5926,
        data2: 0x582E,
        data3: 0x4017,
        data4: [0x9F, 0xDF, 0xE8, 0x99, 0x8D, 0xAA, 0x09, 0x50],
    };

    /// A `<name>.ico` next to the file (e.g. start.bat + start.ico) wins over
    /// the shell icon. Lets you brand .bat/.html tools.
    fn sibling_ico(path: &Path) -> Option<Rgba> {
        let ico = path.with_extension("ico");
        if !ico.is_file() || path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ico")) {
            return None;
        }
        unsafe {
            let w = wide(ico.as_os_str());
            let h = LoadImageW(null_mut(), w.as_ptr(), IMAGE_ICON, 48, 48, LR_LOADFROMFILE);
            if h.is_null() {
                return None;
            }
            let out = hicon_to_rgba(h as HICON);
            DestroyIcon(h as HICON);
            out
        }
    }

    pub fn file_icon(path: &Path) -> Option<Rgba> {
        if let Some(i) = sibling_ico(path) {
            return Some(i);
        }
        unsafe {
            let wp = wide(path.as_os_str());
            let mut info: SHFILEINFOW = std::mem::zeroed();
            // 48px via the system image list, falling back to the 32px large icon.
            let mut hicon: HICON = null_mut();
            if SHGetFileInfoW(
                wp.as_ptr(),
                0,
                &mut info,
                std::mem::size_of::<SHFILEINFOW>() as u32,
                SHGFI_SYSICONINDEX,
            ) != 0
            {
                let mut il: *mut c_void = null_mut();
                if SHGetImageList(SHIL_EXTRALARGE as i32, &IID_IIMAGELIST, &mut il) >= 0 && !il.is_null() {
                    hicon = ImageList_GetIcon(il as HIMAGELIST, info.iIcon, ILD_TRANSPARENT);
                    // SHGetImageList handed us a COM reference: IUnknown::Release is vtable slot 2.
                    let vtbl = *(il as *const *const [unsafe extern "system" fn(*mut c_void) -> u32; 3]);
                    ((*vtbl)[2])(il);
                }
            }
            if hicon.is_null() {
                let mut info: SHFILEINFOW = std::mem::zeroed();
                if SHGetFileInfoW(
                    wp.as_ptr(),
                    0,
                    &mut info,
                    std::mem::size_of::<SHFILEINFOW>() as u32,
                    SHGFI_ICON | SHGFI_LARGEICON,
                ) == 0
                {
                    return None;
                }
                hicon = info.hIcon;
            }
            if hicon.is_null() {
                return None;
            }
            let out = hicon_to_rgba(hicon);
            DestroyIcon(hicon);
            out
        }
    }

    unsafe fn bitmap_bits(hdc: HDC, hbm: HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bmi.bmiHeader.biWidth = w;
        bmi.bmiHeader.biHeight = -h;
        bmi.bmiHeader.biPlanes = 1;
        bmi.bmiHeader.biBitCount = 32;
        bmi.bmiHeader.biCompression = BI_RGB;
        let mut px = vec![0u8; (w * h * 4) as usize];
        let n = GetDIBits(hdc, hbm, 0, h as u32, px.as_mut_ptr() as *mut c_void, &mut bmi, DIB_RGB_COLORS);
        if n == 0 { None } else { Some(px) }
    }

    unsafe fn hicon_to_rgba(hicon: HICON) -> Option<Rgba> {
        let mut ii: ICONINFO = std::mem::zeroed();
        if GetIconInfo(hicon, &mut ii) == 0 {
            return None;
        }
        let result = (|| {
            if ii.hbmColor.is_null() {
                return None;
            }
            let mut bm: BITMAP = std::mem::zeroed();
            if GetObjectW(ii.hbmColor, std::mem::size_of::<BITMAP>() as i32, &mut bm as *mut _ as *mut c_void) == 0 {
                return None;
            }
            let (w, h) = (bm.bmWidth, bm.bmHeight);
            if w <= 0 || h <= 0 || w > 512 || h > 512 {
                return None;
            }
            let hdc = GetDC(null_mut());
            let color = bitmap_bits(hdc, ii.hbmColor, w, h);
            let mask = if ii.hbmMask.is_null() { None } else { bitmap_bits(hdc, ii.hbmMask, w, h) };
            ReleaseDC(null_mut(), hdc);
            let mut px = color?;
            let has_alpha = px.chunks_exact(4).any(|p| p[3] != 0);
            for (i, p) in px.chunks_exact_mut(4).enumerate() {
                p.swap(0, 2); // BGRA -> RGBA
                if !has_alpha {
                    let transparent = mask.as_ref().map_or(false, |m| m[i * 4] != 0);
                    p[3] = if transparent { 0 } else { 255 };
                }
            }
            Some(Rgba { w: w as usize, h: h as usize, px })
        })();
        if !ii.hbmColor.is_null() {
            DeleteObject(ii.hbmColor);
        }
        if !ii.hbmMask.is_null() {
            DeleteObject(ii.hbmMask);
        }
        result
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Rgba;
    use std::path::Path;

    pub fn exe_description(_path: &Path) -> Option<String> {
        None
    }
    pub fn expand_env(s: &str) -> String {
        s.to_string()
    }
    pub fn launch(path: &Path) -> Result<(), String> {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    pub fn reveal(path: &Path) -> Result<(), String> {
        launch(path.parent().unwrap_or(path))
    }
    pub fn cursor_screen_pos() -> Option<(f32, f32)> {
        None
    }
    pub fn file_icon(_path: &Path) -> Option<Rgba> {
        None
    }
    pub fn pick_file(_s: Option<&Path>, _e: &str, _k: bool) -> Option<std::path::PathBuf> {
        None
    }
    pub fn pick_folder() -> Option<std::path::PathBuf> {
        None
    }
    pub const WINDOW_TITLE: &str = "HANGAR // LAUNCH INDEX";
    pub fn acquire_instance() -> bool {
        true
    }
    pub fn focus_existing() {}
    pub fn message_box(title: &str, text: &str) {
        eprintln!("{title}: {text}");
    }
    pub fn thread_init() {}
}

pub fn exe_description(path: &Path) -> Option<String> {
    imp::exe_description(path)
}
pub fn expand_env(s: &str) -> String {
    imp::expand_env(s)
}
pub fn launch(path: &Path) -> Result<(), String> {
    imp::launch(path)
}
pub fn reveal(path: &Path) -> Result<(), String> {
    imp::reveal(path)
}
pub fn cursor_screen_pos() -> Option<(f32, f32)> {
    imp::cursor_screen_pos()
}
pub fn file_icon(path: &Path) -> Option<Rgba> {
    imp::file_icon(path)
}
pub fn pick_file(start: Option<&Path>, ext: &str, keep_lnk: bool) -> Option<std::path::PathBuf> {
    imp::pick_file(start, ext, keep_lnk)
}
pub fn pick_folder() -> Option<std::path::PathBuf> {
    imp::pick_folder()
}
pub const WINDOW_TITLE: &str = imp::WINDOW_TITLE;
pub fn acquire_instance() -> bool {
    imp::acquire_instance()
}
pub fn focus_existing() {
    imp::focus_existing()
}
pub fn message_box(title: &str, text: &str) {
    imp::message_box(title, text)
}
pub fn thread_init() {
    imp::thread_init()
}
