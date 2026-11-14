//! Any bytes as a cover, through the resize applied before embedding. The first
//! byte picks the maximum size, kept small so most decodable images take the
//! resize path rather than the "already fits" one.
#![no_main]

use libfuzzer_sys::fuzz_target;
use tagrex_core::cover::{is_over_max, resize_cover};
use tagrex_core::model::CoverArt;

fuzz_target!(|data: &[u8]| tagrex_fuzz::run(|| {
    let Some((&size, image)) = data.split_first() else {
        return;
    };
    let max_px = u32::from(size % 64) + 1;
    let art = CoverArt {
        mime: "image/jpeg".to_string(),
        data: image.into(),
        ..CoverArt::default()
    };
    let resized = resize_cover(&art, max_px, 90);
    let _ = is_over_max(&resized, max_px);
}));
