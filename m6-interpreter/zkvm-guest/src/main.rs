#![no_std]
#![no_main]
extern crate alloc;
use alloc::vec::Vec;

risc0_zkvm::guest::entry!(main);

use risc0_zkvm::guest::env;
use sbpf_interpreter::{Memory, Syscalls, Vm};

struct GuestSyscalls;
impl Syscalls for GuestSyscalls {
    fn sol_log(&mut self, _: &[u8]) {}
    fn sol_panic(&mut self, _: &[u8], _: u64, _: u64) -> ! { panic!("sol_panic") }
    fn sol_memcpy(&mut self, dst: u64, src: u64, n: u64, mem: &mut Memory) {
        for i in 0..n {
            let b = mem.read_u8(src + i).unwrap_or(0);
            let _ = mem.write_u8(dst + i, b);
        }
    }
    fn sol_memset(&mut self, dst: u64, val: u8, n: u64, mem: &mut Memory) {
        for i in 0..n { let _ = mem.write_u8(dst + i, val); }
    }
    fn sol_memmove(&mut self, dst: u64, src: u64, n: u64, mem: &mut Memory) {
        let bytes: Vec<u8> = (0..n).map(|i| mem.read_u8(src+i).unwrap_or(0)).collect();
        for (i, b) in bytes.into_iter().enumerate() {
            let _ = mem.write_u8(dst + i as u64, b);
        }
    }
}

fn main() {
    let bytecode_len: u32 = env::read();
    let bytecode: Vec<u64> = (0..bytecode_len).map(|_| env::read::<u64>()).collect();
    let buf_len: u32 = env::read();
    let input_buffer: Vec<u8> = (0..buf_len).map(|_| env::read::<u8>()).collect();

    let mut mem = Memory::new(65536, 65536);
    let ptr_slot = mem.heap_base;
    let buf_ptr  = mem.heap_base + 8;
    let _ = mem.write_u64(ptr_slot, buf_ptr);
    for (i, &b) in input_buffer.iter().enumerate() {
        let _ = mem.write_u8(buf_ptr + i as u64, b);
    }

    let mut syscalls = GuestSyscalls;
    let mut vm = Vm::new(&bytecode, mem, &mut syscalls);
    vm.regs[1] = ptr_slot;

    // Commit (r0, success) as a single tuple — decoded together on host
    let result: (u64, bool) = match vm.run() {
        Ok(r0) => (r0, true),
        Err(_)  => (u64::MAX, false),
    };
    env::commit(&result);
}
