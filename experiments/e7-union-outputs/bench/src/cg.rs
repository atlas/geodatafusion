//! Cachegrind client requests (valgrind/cachegrind.h), without a C dependency. Run under
//! `valgrind --tool=cachegrind --instr-at-start=no` to count only the instrumented regions.
//! Outside valgrind the magic sequence is a no-op.

use std::sync::atomic::{AtomicU8, Ordering};

const CG_START: u64 = ((b'C' as u64) << 24) | ((b'G' as u64) << 16);
const CG_STOP: u64 = CG_START + 1;

#[inline(never)]
fn client_request(request: u64) {
    let args: [u64; 6] = [request, 0, 0, 0, 0, 0];
    let mut result: u64 = 0;
    #[cfg(target_arch = "x86_64")]
    unsafe {
        std::arch::asm!(
            "rol rdi, 3",
            "rol rdi, 13",
            "rol rdi, 61",
            "rol rdi, 51",
            "xchg rbx, rbx",
            inout("rdx") result,
            in("rax") args.as_ptr(),
            inout("rdi") 0u64 => _,
            options(nostack),
        );
    }
    std::hint::black_box(result);
}

/// Which region is counted: 0 = none (wall clock only), 1 = the whole query, 2 = kernels only.
pub static REGION: AtomicU8 = AtomicU8::new(0);

pub fn start(region: u8) {
    if REGION.load(Ordering::Relaxed) == region {
        client_request(CG_START);
    }
}

pub fn stop(region: u8) {
    if REGION.load(Ordering::Relaxed) == region {
        client_request(CG_STOP);
    }
}
