# sync_rw_cell

[![Crates.io](https://img.shields.io/crates/v/sync_rw_cell.svg)](https://crates.io/crates/sync_rw_cell)
[![Docs.rs](https://docs.rs/sync_rw_cell/badge.svg)](https://docs.rs/sync_rw_cell)

Defines a `Send` and `Sync` version of `std::cell::RefCell`, which aborts the program
if an attempted borrow fails.