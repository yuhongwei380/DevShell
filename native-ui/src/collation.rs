//! Use the platform locale for the web UI's localeCompare ordering.
pub(super) fn compare(left: &str, right: &str, numeric: bool) -> std::cmp::Ordering {
    #[cfg(target_os = "windows")]
    if let Some(order) = windows_icu::compare(left, right, numeric) {
        return order;
    }
    #[cfg(target_os = "windows")]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CompareStringEx(
                locale: *const u16,
                flags: u32,
                left: *const u16,
                left_len: i32,
                right: *const u16,
                right_len: i32,
                version: *const std::ffi::c_void,
                reserved: *mut std::ffi::c_void,
                sort_handle: isize,
            ) -> i32;
        }
        let left = left.encode_utf16().collect::<Vec<_>>();
        let right = right.encode_utf16().collect::<Vec<_>>();
        let result = unsafe {
            CompareStringEx(
                std::ptr::null(),
                if numeric { 8 } else { 0 },
                left.as_ptr(),
                left.len() as i32,
                right.as_ptr(),
                right.len() as i32,
                std::ptr::null(),
                std::ptr::null_mut(),
                0,
            )
        };
        if result != 0 {
            return result.cmp(&2);
        }
    }
    #[cfg(target_os = "macos")]
    {
        use std::ffi::c_void;
        #[repr(C)]
        struct Range {
            location: isize,
            length: isize,
        }
        #[link(name = "CoreFoundation", kind = "framework")]
        unsafe extern "C" {
            fn CFStringCreateWithBytes(
                allocator: *const c_void,
                bytes: *const u8,
                length: isize,
                encoding: u32,
                external: bool,
            ) -> *const c_void;
            fn CFLocaleCopyCurrent() -> *const c_void;
            fn CFStringCompareWithOptionsAndLocale(
                left: *const c_void,
                right: *const c_void,
                range: Range,
                options: usize,
                locale: *const c_void,
            ) -> isize;
            fn CFRelease(value: *const c_void);
        }
        unsafe {
            let a = CFStringCreateWithBytes(
                std::ptr::null(),
                left.as_ptr(),
                left.len() as isize,
                0x08000100,
                false,
            );
            let b = CFStringCreateWithBytes(
                std::ptr::null(),
                right.as_ptr(),
                right.len() as isize,
                0x08000100,
                false,
            );
            let locale = CFLocaleCopyCurrent();
            let result = if !a.is_null() && !b.is_null() && !locale.is_null() {
                Some(
                    CFStringCompareWithOptionsAndLocale(
                        a,
                        b,
                        Range {
                            location: 0,
                            length: left.encode_utf16().count() as isize,
                        },
                        if numeric { 64 } else { 0 },
                        locale,
                    )
                    .cmp(&0),
                )
            } else {
                None
            };
            for value in [a, b, locale] {
                if !value.is_null() {
                    CFRelease(value);
                }
            }
            if let Some(result) = result {
                return result;
            }
        }
    }
    left.to_lowercase().cmp(&right.to_lowercase())
}

#[cfg(target_os = "windows")]
mod windows_icu {
    use std::{
        cell::RefCell,
        ffi::{CString, c_char, c_void},
        sync::OnceLock,
    };
    type Open = unsafe extern "C" fn(*const c_char, *mut i32) -> *mut c_void;
    type Close = unsafe extern "C" fn(*mut c_void);
    type Attribute = unsafe extern "C" fn(*mut c_void, i32, i32, *mut i32);
    type Compare = unsafe extern "C" fn(
        *const c_void,
        *const c_char,
        i32,
        *const c_char,
        i32,
        *mut i32,
    ) -> i32;
    struct Api {
        open: Open,
        close: Close,
        attribute: Attribute,
        compare: Compare,
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryExW(name: *const u16, reserved: *mut c_void, flags: u32) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn GetUserDefaultLocaleName(name: *mut u16, count: i32) -> i32;
    }
    fn api() -> Option<&'static Api> {
        static API: OnceLock<Option<Api>> = OnceLock::new();
        API.get_or_init(|| unsafe {
            // Use the OS ICU, as Chromium does for Intl.Collator. NLS differs
            // in script ordering (notably Han versus Latin) even for zh-CN.
            let name = "icu.dll\0".encode_utf16().collect::<Vec<_>>();
            let module = LoadLibraryExW(name.as_ptr(), std::ptr::null_mut(), 0x800);
            if module.is_null() {
                return None;
            }
            let get = |name: &[u8]| {
                let address = GetProcAddress(module, name.as_ptr());
                if address.is_null() {
                    None
                } else {
                    Some(address)
                }
            };
            Some(Api {
                open: std::mem::transmute::<*mut c_void, Open>(get(b"ucol_open\0")?),
                close: std::mem::transmute::<*mut c_void, Close>(get(b"ucol_close\0")?),
                attribute: std::mem::transmute::<*mut c_void, Attribute>(get(
                    b"ucol_setAttribute\0",
                )?),
                compare: std::mem::transmute::<*mut c_void, Compare>(get(b"ucol_strcollUTF8\0")?),
            })
        })
        .as_ref()
    }
    struct Collators {
        normal: *mut c_void,
        numeric: *mut c_void,
        api: &'static Api,
    }
    impl Collators {
        fn new() -> Option<Self> {
            let api = api()?;
            let mut name = [0u16; 85];
            let count = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), 85) };
            let locale = if count > 1 {
                String::from_utf16_lossy(&name[..count as usize - 1]).replace('-', "_")
            } else {
                "zh_CN".into()
            };
            let locale = CString::new(locale).ok()?;
            let mut error = 0;
            let normal = unsafe { (api.open)(locale.as_ptr(), &mut error) };
            if error > 0 || normal.is_null() {
                return None;
            }
            error = 0;
            let numeric = unsafe { (api.open)(locale.as_ptr(), &mut error) };
            if error > 0 || numeric.is_null() {
                unsafe {
                    (api.close)(normal);
                }
                return None;
            }
            unsafe {
                (api.attribute)(numeric, 7, 17, &mut error);
            }
            if error > 0 {
                unsafe {
                    (api.close)(normal);
                    (api.close)(numeric);
                }
                return None;
            }
            Some(Self {
                normal,
                numeric,
                api,
            })
        }
    }
    impl Drop for Collators {
        fn drop(&mut self) {
            unsafe {
                (self.api.close)(self.normal);
                (self.api.close)(self.numeric);
            }
        }
    }
    #[cfg(test)]
    mod tests {
        #[test]
        fn chinese_script_order_matches_intl_instead_of_nls() {
            let api = super::api().unwrap();
            let mut error = 0;
            let locale = std::ffi::CString::new("zh_CN").unwrap();
            let collator = unsafe { (api.open)(locale.as_ptr(), &mut error) };
            assert!(!collator.is_null() && error <= 0);
            error = 0;
            let left = "部署说明.txt";
            let right = "app.js";
            let result = unsafe {
                (api.compare)(
                    collator,
                    left.as_ptr().cast(),
                    left.len() as i32,
                    right.as_ptr().cast(),
                    right.len() as i32,
                    &mut error,
                )
            };
            unsafe {
                (api.close)(collator);
            }
            assert!(result < 0 && error <= 0);
        }
    }
    thread_local! {static COLLATOR:RefCell<Option<Collators>>=const{RefCell::new(None)};}
    pub fn compare(left: &str, right: &str, numeric: bool) -> Option<std::cmp::Ordering> {
        COLLATOR.with(|cache| {
            let mut cache = cache.borrow_mut();
            if cache.is_none() {
                *cache = Collators::new();
            }
            let collators = cache.as_ref()?;
            let mut error = 0;
            let result = unsafe {
                (collators.api.compare)(
                    if numeric {
                        collators.numeric
                    } else {
                        collators.normal
                    },
                    left.as_ptr().cast(),
                    left.len() as i32,
                    right.as_ptr().cast(),
                    right.len() as i32,
                    &mut error,
                )
            };
            if error > 0 {
                None
            } else {
                Some(result.cmp(&0))
            }
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn numeric_tree_order_keeps_double_digit_names_together() {
        assert!(super::compare("file2", "file10", true).is_lt());
        assert!(super::compare("file2", "file10", false).is_gt());
    }
}
