//! macOS glue without extra crates: files opened from Finder ("Öffnen mit", double-click,
//! dropping on the Dock icon) and moving files to the Trash.
//!
//! Files opened from Finder:
//! macOS delivers them to the application delegate, not as command-line arguments, so we
//! add `application:openURLs:` to winit's delegate class through the Objective-C runtime
//! and queue the paths for the app to pick up on its next frame.

use std::path::PathBuf;
use std::sync::Mutex;

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Paths Finder asked us to open since the last call.
pub fn take() -> Vec<PathBuf> { OPENED.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default() }

#[cfg(target_os = "macos")]
mod imp {
    use std::ffi::{c_char, c_void, CStr};
    type Id = *mut c_void;
    type Sel = *mut c_void;

    #[link(name = "objc")]
    extern "C" {
        fn objc_getClass(name: *const c_char) -> Id;
        fn sel_registerName(name: *const c_char) -> Sel;
        fn objc_msgSend();
        fn class_addMethod(cls: Id, name: Sel, imp: *const c_void, types: *const c_char) -> bool;
    }
    #[link(name = "AppKit", kind = "framework")]
    extern "C" {}

    fn sel(s: &CStr) -> Sel { unsafe { sel_registerName(s.as_ptr()) } }

    unsafe fn send_ptr(obj: Id, s: Sel, p: *const c_char) -> Id {
        let f: unsafe extern "C" fn(Id, Sel, *const c_char) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f(obj, s, p)
    }

    unsafe fn send_id(obj: Id, s: Sel, a: Id) -> Id {
        let f: unsafe extern "C" fn(Id, Sel, Id) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f(obj, s, a)
    }

    /// `[[NSFileManager defaultManager] trashItemAtURL:…]`; returns where the item went.
    pub fn trash(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
        let c = std::ffi::CString::new(path.to_string_lossy().as_bytes()).map_err(|e| e.to_string())?;
        unsafe {
            let s = send_ptr(objc_getClass(c"NSString".as_ptr()), sel(c"stringWithUTF8String:"), c.as_ptr());
            let url = send_id(objc_getClass(c"NSURL".as_ptr()), sel(c"fileURLWithPath:"), s);
            let fm = send0(objc_getClass(c"NSFileManager".as_ptr()), sel(c"defaultManager"));
            if url.is_null() || fm.is_null() { return Err("Papierkorb nicht verfügbar".into()); }
            let mut out: Id = std::ptr::null_mut();
            let mut err: Id = std::ptr::null_mut();
            let f: unsafe extern "C" fn(Id, Sel, Id, *mut Id, *mut Id) -> i8 = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
            if f(fm, sel(c"trashItemAtURL:resultingItemURL:error:"), url, &mut out, &mut err) == 0 {
                let msg = if err.is_null() { None } else { nsstring(send0(err, sel(c"localizedDescription"))) };
                return Err(msg.unwrap_or_else(|| "Unbekannter Fehler".into()));
            }
            Ok(if out.is_null() { std::path::PathBuf::new() } else { nsstring(send0(out, sel(c"path"))).map(Into::into).unwrap_or_default() })
        }
    }

    unsafe fn send0(obj: Id, s: Sel) -> Id {
        let f: unsafe extern "C" fn(Id, Sel) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f(obj, s)
    }

    unsafe fn send_usize(obj: Id, s: Sel, i: usize) -> Id {
        let f: unsafe extern "C" fn(Id, Sel, usize) -> Id = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f(obj, s, i)
    }

    unsafe fn count(obj: Id) -> usize {
        let f: unsafe extern "C" fn(Id, Sel) -> usize = std::mem::transmute(objc_msgSend as unsafe extern "C" fn());
        f(obj, sel(c"count"))
    }

    unsafe fn nsstring(s: Id) -> Option<String> {
        if s.is_null() { return None; }
        let p = send0(s, sel(c"UTF8String")) as *const c_char;
        if p.is_null() { None } else { Some(CStr::from_ptr(p).to_string_lossy().into_owned()) }
    }

    extern "C" fn open_urls(_this: Id, _cmd: Sel, _app: Id, urls: Id) {
        unsafe {
            for i in 0..count(urls) {
                let url = send_usize(urls, sel(c"objectAtIndex:"), i);
                if let Some(p) = nsstring(send0(url, sel(c"path"))) {
                    if std::env::var("FREO_DEBUG_OPEN").is_ok() { eprintln!("open-url {}", p); }
                    if let Ok(mut v) = super::OPENED.lock() { v.push(p.into()); }
                }
            }
        }
    }

    /// Call before the event loop starts. On a cold launch AppKit hands over the files
    /// before `applicationDidFinishLaunching`, earlier than any eframe callback, and the
    /// delegate class does not exist yet — so the method goes onto NSObject, where the
    /// delegate inherits it (AppKit only asks the delegate whether it responds).
    pub fn install() {
        unsafe {
            let root = objc_getClass(c"NSObject".as_ptr());
            if root.is_null() { return; }
            let ok = class_addMethod(root, sel(c"application:openURLs:"), open_urls as *const c_void, c"v@:@@".as_ptr());
            if std::env::var("FREO_DEBUG_OPEN").is_ok() { eprintln!("open-url handler installed: {}", ok); }
        }
    }
}

#[cfg(target_os = "macos")]
pub use imp::{install, trash};

#[cfg(not(target_os = "macos"))]
pub fn install() {}

#[cfg(not(target_os = "macos"))]
pub fn trash(_path: &std::path::Path) -> Result<std::path::PathBuf, String> { Err("Der Papierkorb wird nur unter macOS unterstützt.".into()) }
