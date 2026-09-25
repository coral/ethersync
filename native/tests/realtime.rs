use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use tidkod::*;
struct CountAlloc;
thread_local! {static ENABLED:Cell<bool>=const{Cell::new(false)};static COUNT:Cell<usize>=const{Cell::new(0)};}
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if ENABLED.try_with(Cell::get).unwrap_or(false) {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
        // SAFETY: forwards the original allocator layout unchanged to System.
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, p: *mut u8, layout: Layout) {
        unsafe { System.dealloc(p, layout) }
    }
    unsafe fn realloc(&self, p: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        if ENABLED.try_with(Cell::get).unwrap_or(false) {
            let _ = COUNT.try_with(|c| c.set(c.get() + 1));
        }
        unsafe { System.realloc(p, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;
#[test]
fn reader_is_allocation_free_and_slots_are_reclaimed() {
    let engine = Engine::new().unwrap();
    let leader = engine
        .leader(LeaderConfig {
            bind: "127.0.0.1:0".parse().unwrap(),
            advertise: false,
            ..Default::default()
        })
        .unwrap();
    for _ in 0..100 {
        drop(leader.reader().unwrap());
    }
    leader.play().unwrap();
    let mut r = leader.reader().unwrap();
    let base = engine.clock().now_ns();
    r.read_at(base);
    COUNT.with(|c| c.set(0));
    ENABLED.with(|c| c.set(true));
    for i in 0..100_000 {
        std::hint::black_box(r.read_at(base + i * 1000));
        let snapshot = r.snapshot();
        std::hint::black_box(snapshot.evaluate(base + i * 1000));
        std::hint::black_box(snapshot.evaluate_sample(base, i, 48000).unwrap());
        std::hint::black_box(snapshot.evaluate_for_presentation(base, i * 1000).unwrap());
        std::hint::black_box(
            r.read_for_presentation_at(base + i * 1000, std::time::Duration::from_millis(20))
                .unwrap(),
        );
        if i < 1000 {
            std::hint::black_box(r.next_boundary_at(base + i * 1000));
            std::hint::black_box(snapshot.next_boundary(base + i * 1000));
        }
    }
    ENABLED.with(|c| c.set(false));
    assert_eq!(COUNT.with(Cell::get), 0);
    engine.shutdown().unwrap();
    assert_eq!(r.read().status.connection, ConnectionState::Shutdown);
}
