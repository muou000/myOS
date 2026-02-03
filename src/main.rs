#![no_std]
#![no_main]

#[macro_use]
extern crate axlog;
extern crate axruntime;

#[unsafe(no_mangle)]
fn main() {
    info!("Hello, MyOS!");

    loop {}
}