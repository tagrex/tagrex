//! WebKitGTK 2.54 can flash a pre-animation frame at the end of a fade.
//! Keep GPU acceleration, but use TextureMapper and whole-frame redraws on
//! this affected series. Native regression: tests/ui/native-rendering.py.
//! Related reproduction: https://github.com/SimoHypers/limusic/issues/433
//! Recheck that probe before removing this workaround for a WebKit update.

use std::ffi::{c_char, c_int, c_void, CStr};
use webkit2gtk::{
    glib::{translate::ToGlibPtr, ObjectExt},
    WebProcessTerminationReason, WebViewExt,
};

pub fn install(window: &tauri::WebviewWindow) {
    // An explicit developer choice takes precedence. Older/newer WebKit series
    // retain their defaults; this was reproduced on 2.54.1.
    if std::env::var_os("WEBKIT_USE_SKIA_FOR_COMPOSITION").is_some() {
        return;
    }
    if let Err(error) = window.with_webview(|platform| {
        let webview = platform.inner();
        // SAFETY: these version getters take no arguments and own no data.
        let affected = unsafe {
            webkit2gtk::ffi::webkit_get_major_version() == 2
                && webkit2gtk::ffi::webkit_get_minor_version() == 54
        };
        if !affected {
            return;
        }
        let (Some(settings), Some(uri)) = (webview.settings(), webview.uri()) else {
            return;
        };
        if !disable_affected_features(&settings) {
            return;
        }
        // Wry has already started the initial load. These preferences are read
        // when the page is created, so recreate it once before the user works.
        // Disconnect first: subsequent web-process termination is not ours.
        let handler = std::rc::Rc::new(std::cell::RefCell::new(None));
        let pending = handler.clone();
        *handler.borrow_mut() =
            Some(webview.connect_web_process_terminated(move |view, reason| {
                if let Some(id) = pending.borrow_mut().take() {
                    view.disconnect(id);
                }
                if reason == WebProcessTerminationReason::TerminatedByApi {
                    view.load_uri(&uri);
                }
            }));
        webview.terminate_web_process();
        eprintln!("WebKitGTK 2.54: GPU TextureMapper enabled; partial compositing disabled");
    }) {
        eprintln!("Could not configure WebKit compositor: {error}");
    }
}

fn disable_affected_features(settings: &webkit2gtk::Settings) -> bool {
    // The Rust bindings predate WebKitFeature. Resolve the public 2.42+ API
    // from the already-linked library so older supported systems still load.
    // Signatures/ownership: WebKitSettings.h and WebKitFeature.h. The list is
    // owned; its feature pointers are borrowed only until list_unref.
    unsafe {
        macro_rules! symbol {
            ($name:expr, $signature:ty) => {{
                let address = libc::dlsym(libc::RTLD_DEFAULT, $name.as_ptr());
                if address.is_null() {
                    return false;
                }
                std::mem::transmute::<*mut c_void, $signature>(address)
            }};
        }
        let all = symbol!(
            c"webkit_settings_get_all_features",
            unsafe extern "C" fn() -> *mut c_void
        );
        let length = symbol!(
            c"webkit_feature_list_get_length",
            unsafe extern "C" fn(*mut c_void) -> usize
        );
        let get = symbol!(
            c"webkit_feature_list_get",
            unsafe extern "C" fn(*mut c_void, usize) -> *mut c_void
        );
        let identifier = symbol!(
            c"webkit_feature_get_identifier",
            unsafe extern "C" fn(*mut c_void) -> *const c_char
        );
        let enabled = symbol!(
            c"webkit_settings_get_feature_enabled",
            unsafe extern "C" fn(*mut c_void, *mut c_void) -> c_int
        );
        let set = symbol!(
            c"webkit_settings_set_feature_enabled",
            unsafe extern "C" fn(*mut c_void, *mut c_void, c_int)
        );
        let unref = symbol!(
            c"webkit_feature_list_unref",
            unsafe extern "C" fn(*mut c_void)
        );
        let list = all();
        if list.is_null() {
            return false;
        }
        let raw: *mut webkit2gtk::ffi::WebKitSettings = settings.to_glib_none().0;
        let mut changed = false;
        for i in 0..length(list) {
            let feature = get(list, i);
            if feature.is_null() {
                continue;
            }
            let name = identifier(feature);
            if !name.is_null()
                && matches!(
                    CStr::from_ptr(name).to_bytes(),
                    b"UseSkiaForComposition"
                        | b"UseDamagingInformationForCompositing"
                        | b"PropagateDamagingInformation"
                )
                && enabled(raw.cast(), feature) != 0
            {
                set(raw.cast(), feature, 0);
                changed = true;
            }
        }
        unref(list);
        changed
    }
}
