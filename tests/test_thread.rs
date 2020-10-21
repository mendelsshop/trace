extern crate trace;

#[macro_use]
mod trace_test;

use std::thread;
use std::time::Duration;
use trace::trace;

trace::init_depth_var!();

trace_test!(test_thread, {
    let handle = thread::spawn(|| {
        foo(10);
    });

    bar(20);

    handle.join().unwrap();
});

#[trace]
fn foo(x: u32) -> u32 {
    thread::sleep(Duration::from_millis(100));
    bar(x + 2) - 4
}

#[trace]
fn bar(x: u32) -> u32 {
    thread::sleep(Duration::from_millis(200));
    x + 10
}
