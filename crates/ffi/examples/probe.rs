fn main() {
    let arg = std::env::args().nth(1).expect("usage: probe <folder>");
    let c = std::ffi::CString::new(arg).unwrap();
    let ptr = unsafe { tagrex_ffi::tagrex_scan_json(c.as_ptr()) };
    let text = unsafe { std::ffi::CStr::from_ptr(ptr) }
        .to_str()
        .unwrap()
        .to_owned();
    unsafe { tagrex_ffi::tagrex_string_free(ptr) };
    println!("{text}");
}
