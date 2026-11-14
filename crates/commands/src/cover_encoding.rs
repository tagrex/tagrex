//! Base64 artwork is shared for the lifetime of one preview request.
//!
//! A SHA-256 content key reuses one encoded string even for equal images read
//! into different allocations. Strong strings live until the outer Scope ends;
//! the response then owns its clones. There is no cross-request cache.

use base64::Engine as _;
use sha2::{Digest, Sha256};
use std::{cell::RefCell, collections::HashMap, sync::Arc};

type Pool = HashMap<[u8; 32], Arc<str>>;

thread_local! {
    static POOL: RefCell<Option<Pool>> = const { RefCell::new(None) };
}

/// Own the outermost request's encoding map; nested helpers share its lifetime.
pub(crate) struct Scope {
    owns: bool,
}

impl Scope {
    /// Start a request map, or join an already-active request on this thread.
    pub fn new() -> Self {
        let owns = POOL.with(|pool| {
            let mut pool = pool.borrow_mut();
            if pool.is_none() {
                *pool = Some(Pool::new());
                true
            } else {
                false
            }
        });
        Self { owns }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if self.owns {
            POOL.with(|pool| {
                pool.borrow_mut().take();
            });
        }
    }
}

/// Encode once per content digest during a request. Outside a request, encode
/// directly rather than retaining data until an unrelated future operation.
pub(crate) fn encode(data: &Arc<[u8]>) -> Arc<str> {
    let encode = || -> Arc<str> {
        base64::engine::general_purpose::STANDARD
            .encode(data)
            .into()
    };
    POOL.with(|pool| match pool.borrow_mut().as_mut() {
        Some(pool) => pool
            .entry(Sha256::digest(data).into())
            .or_insert_with(encode)
            .clone(),
        None => encode(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode(encoded: &str) -> Vec<u8> {
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap()
    }

    #[test]
    fn separately_allocated_equal_bytes_share_one_encoding() {
        let _scope = Scope::new();
        let first: Arc<[u8]> = vec![7; 4096].into();
        let second: Arc<[u8]> = vec![7; 4096].into();
        assert!(!Arc::ptr_eq(&first, &second));
        let a = encode(&first);
        let b = encode(&second);
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(decode(&a), first.as_ref());
    }

    #[test]
    fn mutated_bytes_get_their_own_encoding() {
        let _scope = Scope::new();
        let mut data: Arc<[u8]> = vec![1; 128].into();
        let before = encode(&data);
        Arc::make_mut(&mut data)[0] = 2;
        let after = encode(&data);
        assert_ne!(before, after);
        assert_eq!(decode(&before), vec![1; 128]);
        assert_eq!(decode(&after), data.as_ref());
    }

    #[test]
    fn nested_scope_cannot_end_its_parents_pool() {
        let outer = Scope::new();
        let data: Arc<[u8]> = vec![3; 32].into();
        let first = encode(&data);
        {
            let _inner = Scope::new();
            assert!(Arc::ptr_eq(&first, &encode(&data)));
        }
        assert!(Arc::ptr_eq(&first, &encode(&data)));
        drop(outer);
        assert_eq!(Arc::strong_count(&first), 1);
    }

    #[test]
    fn ending_a_request_releases_the_pool_but_keeps_the_response_valid() {
        let data: Arc<[u8]> = vec![4; 32].into();
        let first = {
            let _scope = Scope::new();
            encode(&data)
        };
        assert_eq!(Arc::strong_count(&first), 1);
        let second = {
            let _scope = Scope::new();
            encode(&data)
        };
        assert!(!Arc::ptr_eq(&first, &second));
        assert_eq!(first, second);
        assert_eq!(decode(&first), data.as_ref());
    }

    #[test]
    fn no_scope_means_no_retained_cache() {
        let data: Arc<[u8]> = vec![5; 32].into();
        let first = encode(&data);
        let second = encode(&data);
        assert_eq!(Arc::strong_count(&first), 1);
        assert_eq!(first, second);
        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn empty_and_large_images_are_not_subject_to_an_eviction_threshold() {
        let _scope = Scope::new();
        for data in [Arc::<[u8]>::from([]), Arc::from(vec![9; 2 * 1024 * 1024])] {
            let first = encode(&data);
            assert!(Arc::ptr_eq(&first, &encode(&data)));
            assert_eq!(decode(&first), data.as_ref());
        }
    }
}
