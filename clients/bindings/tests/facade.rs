use ethersync_bindings::api::*;
#[test]
fn timecode_arithmetic_is_exact_and_validates_labels() {
    assert!(timecode_format_new(25, 1, true).is_err());
    let f = timecode_format_new(30000, 1001, true).unwrap();
    assert!(timecode_format_position(&f, 0, 1, 0, 0).is_err());
    assert_eq!(
        timecode_format_position(&f, 0, 1, 0, 2).unwrap().frames,
        1800
    );
    assert_eq!(
        timecode_format_position(&f, 0, 10, 0, 0).unwrap().frames,
        17982
    );
    assert!(timecode_format_position(&f, 24, 0, 0, 0).is_err());
    let p = timecode_format_elapsed(&f, 1_001_000_000);
    assert_eq!((p.frames, p.subframe), (30, 0));
    let f = timecode_format_new(30, 1, false).unwrap();
    let p = timecode_format_elapsed(&f, -50_000_000);
    assert_eq!((p.frames, p.subframe), (-2, 0x80000000));
    assert_eq!(
        timecode_format_elapsed(&f, 86_400_000_000_000).frames,
        2_592_000
    );
}
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};
struct Alloc;
thread_local! {static ENABLED:Cell<bool>=const{Cell::new(false)};static COUNT:Cell<usize>=const{Cell::new(0)};}
unsafe impl GlobalAlloc for Alloc {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        if ENABLED.try_with(Cell::get).unwrap_or(false) {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        if ENABLED.try_with(Cell::get).unwrap_or(false) {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static ALLOC: Alloc = Alloc;
#[test]
fn exact_large_position_and_allocation_free_portable_reads() {
    let core = core_configured(i64::MAX - 7, u32::MAX, 30, 1, false, 0.03, 1., 3).unwrap();
    let value = core_read(&core, 0);
    assert_eq!(value.frames, i64::MAX - 7);
    assert_eq!(value.subframe, u32::MAX);
    let mut snapshot = timecode_snapshot_new();
    let copy = timecode_snapshot_copy(&core_snapshot(&core));
    COUNT.with(|c| c.set(0));
    ENABLED.with(|c| c.set(true));
    for i in 0..100_000 {
        std::hint::black_box(core_read(&core, i));
        std::hint::black_box(core_next_boundary(&core, i));
        core_snapshot_into(&core, &mut snapshot);
        std::hint::black_box(timecode_snapshot_read(&snapshot, i));
        std::hint::black_box(timecode_snapshot_read_for_presentation(&snapshot, i, 1).unwrap());
        std::hint::black_box(timecode_snapshot_next_boundary(&snapshot, i));
    }
    ENABLED.with(|c| c.set(false));
    assert_eq!(COUNT.with(Cell::get), 0);
    drop(core);
    assert_eq!(timecode_snapshot_read(&copy, 100).frames, i64::MAX - 7);
    assert!(timecode_snapshot_read_for_presentation(&copy, i64::MAX as u64, 1).is_err());
}
#[cfg(feature = "native")]
#[test]
fn control_acknowledges_reader_publication_and_children_own_engine() {
    let engine = engine_new().unwrap();
    let mut options = leader_options_new();
    leader_options_advertise(&mut options, false);
    let leader = engine_leader(&engine, &options).unwrap();
    let mut reader = leader_reader(&leader).unwrap();
    let endpoints = leader_local_endpoints(&leader).unwrap();
    assert!(endpoint_list_count(&endpoints) > 0);
    assert!(endpoint_list_get(&endpoints, endpoint_list_count(&endpoints)).is_err());
    drop(engine); // The leader retains its worker owner.
    for frame in 1..1000 {
        leader_seek(&leader, frame, 0x12345678).unwrap();
        let r = reader_read(&mut reader);
        assert_eq!(r.frames, frame);
        assert_eq!(r.subframe, 0x12345678);
    }
    let mut snapshot = reader_snapshot(&mut reader);
    let copied = timecode_snapshot_copy(&snapshot);
    leader_seek(&leader, -7, 0x80000000).unwrap();
    COUNT.with(|c| c.set(0));
    ENABLED.with(|c| c.set(true));
    for _ in 0..100_000 {
        std::hint::black_box(reader_read(&mut reader));
        reader_snapshot_into(&mut reader, &mut snapshot);
        std::hint::black_box(timecode_snapshot_read(&snapshot, 0));
    }
    ENABLED.with(|c| c.set(false));
    assert_eq!(COUNT.with(Cell::get), 0);
    leader_shutdown(&leader).unwrap();
    drop((reader, leader));
    assert_eq!(timecode_snapshot_read(&copied, 0).frames, 999);
    assert_eq!(timecode_snapshot_read(&snapshot, 0).frames, -7);
}

#[cfg(feature = "native")]
#[test]
fn typed_endpoints_validate_and_preserve_ipv6_scope() {
    assert!(endpoint_ipv4(&[127, 0, 0], 4443).is_err());
    assert!(endpoint_ipv4(&[127, 0, 0, 1], 65536).is_err());
    assert!(endpoint_ipv6(&[0; 15], 4443, 0).is_err());
    assert!(endpoint_parse("host.invalid:4443").is_err());
    let endpoint = endpoint_parse("[fe80::1%7]:4443").unwrap();
    assert_eq!(endpoint_port(&endpoint), 4443);
    assert_eq!(endpoint_address(&endpoint), "[fe80::1%7]:4443");
    assert!(follower_options_endpoint(&endpoint).is_ok());
    assert!(follower_options_endpoint(&endpoint_loopback(0).unwrap()).is_err());
    assert!(follower_options_endpoint(&endpoint_any_ipv4(4443).unwrap()).is_err());
    let mut options = leader_options_new();
    leader_options_bind_endpoint(&mut options, &endpoint_loopback(0).unwrap());
}
