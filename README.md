# aware
Simple process supervisor written in Rust

### How does it work?
It looks to see if the program has terminated with an error. If it did, it restarts it.
It also logs changes, which will help in debugging.

### How to build?
Clone this repo and run `rustc aware/aware-0.1-4-rust.rs --crate-name aware`

### Usage:
To supervise a program: `./aware supervise /path/to/program [arg1 arg2 ...]`
To stop a specific supervised program: `./aware leave program_name`
To stop all supervised programs: `./aware leave`
