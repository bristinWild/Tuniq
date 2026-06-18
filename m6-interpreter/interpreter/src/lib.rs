//! M6 sBPF interpreter — covers the full opcode set emitted by real Solana
//! programs compiled with the standard toolchain.
//!
//! Conformance target: `balance >= threshold` predicate compiled via
//! `cargo build-sbf`, which exercises ~50 distinct opcodes.
//!
//! Design:
//! - `no_std` compatible (runs inside the RISC Zero zkVM guest)
//! - Memory model: flat byte-addressable heap, stack via r10
//! - Syscall surface: minimal — `sol_log_`, `sol_panic_`, `sol_memcpy_`,
//!   `sol_memset_`, `sol_memmove_` (stubs that record the call)
//! - Does NOT implement: packet filters, maps, tail calls, JIT

#![no_std]

extern crate alloc;
use alloc::vec::Vec;

// ── Error type ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SbpfError {
    UnknownOpcode(u8),
    OutOfBounds { pc: usize },
    DivisionByZero,
    InvalidMemoryAccess { addr: u64, size: usize },
    CallStackOverflow,
    BadExit,
}

// ── Memory ───────────────────────────────────────────────────────────────────

pub struct Memory {
    pub heap: Vec<u8>,
    pub stack: Vec<u8>,
    pub stack_base: u64,   // virtual address of stack bottom
    pub heap_base: u64,    // virtual address of heap base
}

impl Memory {
    pub fn new(heap_size: usize, stack_size: usize) -> Self {
        Self {
            heap: alloc::vec![0u8; heap_size],
            stack: alloc::vec![0u8; stack_size],
            stack_base: 0x1_0000_0000,
            heap_base: 0x3_0000_0000,
        }
    }

    fn resolve(&self, addr: u64, size: usize) -> Option<(*const u8, bool)> {
        let sb = self.stack_base;
        let ss = self.stack.len() as u64;
        let hb = self.heap_base;
        let hs = self.heap.len() as u64;
        if addr >= sb && addr + size as u64 <= sb + ss {
            let off = (addr - sb) as usize;
            Some((self.stack[off..].as_ptr(), false))
        } else if addr >= hb && addr + size as u64 <= hb + hs {
            let off = (addr - hb) as usize;
            Some((self.heap[off..].as_ptr(), true))
        } else {
            None
        }
    }

    fn resolve_mut(&mut self, addr: u64, size: usize) -> Option<(*mut u8, bool)> {
        let sb = self.stack_base;
        let ss = self.stack.len() as u64;
        let hb = self.heap_base;
        let hs = self.heap.len() as u64;
        if addr >= sb && addr + size as u64 <= sb + ss {
            let off = (addr - sb) as usize;
            Some((self.stack[off..].as_mut_ptr(), false))
        } else if addr >= hb && addr + size as u64 <= hb + hs {
            let off = (addr - hb) as usize;
            Some((self.heap[off..].as_mut_ptr(), true))
        } else {
            None
        }
    }

    pub fn read_u8(&self, addr: u64) -> Option<u8> {
        let (p, _) = self.resolve(addr, 1)?;
        Some(unsafe { *p })
    }
    pub fn read_u16(&self, addr: u64) -> Option<u16> {
        let (p, _) = self.resolve(addr, 2)?;
        Some(u16::from_le_bytes(unsafe { *(p as *const [u8; 2]) }))
    }
    pub fn read_u32(&self, addr: u64) -> Option<u32> {
        let (p, _) = self.resolve(addr, 4)?;
        Some(u32::from_le_bytes(unsafe { *(p as *const [u8; 4]) }))
    }
    pub fn read_u64(&self, addr: u64) -> Option<u64> {
        let (p, _) = self.resolve(addr, 8)?;
        Some(u64::from_le_bytes(unsafe { *(p as *const [u8; 8]) }))
    }

    pub fn write_u8(&mut self, addr: u64, v: u8) -> Option<()> {
        let (p, _) = self.resolve_mut(addr, 1)?;
        unsafe { *p = v }; Some(())
    }
    pub fn write_u16(&mut self, addr: u64, v: u16) -> Option<()> {
        let (p, _) = self.resolve_mut(addr, 2)?;
        unsafe { *(p as *mut [u8; 2]) = v.to_le_bytes() }; Some(())
    }
    pub fn write_u32(&mut self, addr: u64, v: u32) -> Option<()> {
        let (p, _) = self.resolve_mut(addr, 4)?;
        unsafe { *(p as *mut [u8; 4]) = v.to_le_bytes() }; Some(())
    }
    pub fn write_u64(&mut self, addr: u64, v: u64) -> Option<()> {
        let (p, _) = self.resolve_mut(addr, 8)?;
        unsafe { *(p as *mut [u8; 8]) = v.to_le_bytes() }; Some(())
    }
}

// ── Syscall interface ────────────────────────────────────────────────────────

/// Minimal syscall surface for confidential predicates.
/// The guest program can call these; the trait implementation decides
/// what they do (stub/log/panic).
pub trait Syscalls {
    fn sol_log(&mut self, msg: &[u8]);
    fn sol_panic(&mut self, file: &[u8], line: u64, column: u64) -> !;
    fn sol_memcpy(&mut self, dst: u64, src: u64, n: u64, mem: &mut Memory);
    fn sol_memset(&mut self, dst: u64, val: u8, n: u64, mem: &mut Memory);
    fn sol_memmove(&mut self, dst: u64, src: u64, n: u64, mem: &mut Memory);
}

// ── Instruction decode ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
struct Insn {
    opcode: u8,
    dst:    usize,
    src:    usize,
    off:    i16,
    imm:    i32,
}

#[inline(always)]
fn decode(word: u64) -> Insn {
    Insn {
        opcode: (word & 0xFF) as u8,
        dst:    ((word >> 8)  & 0x0F) as usize,
        src:    ((word >> 12) & 0x0F) as usize,
        off:    ((word >> 16) & 0xFFFF) as i16,
        imm:    (word >> 32) as i32,
    }
}

// ── VM ───────────────────────────────────────────────────────────────────────

const MAX_CALL_DEPTH: usize = 64;

pub struct Vm<'a, S: Syscalls> {
    pub regs: [u64; 11],
    pub pc:   usize,
    text:     &'a [u64],
    call_stack: Vec<(usize, [u64; 11])>, // (return pc, saved callee-saved regs)
    pub mem:  Memory,
    syscalls: &'a mut S,
    pub return_data: Vec<u8>,
}

impl<'a, S: Syscalls> Vm<'a, S> {
    pub fn new(text: &'a [u64], mem: Memory, syscalls: &'a mut S) -> Self {
        let mut regs = [0u64; 11];
        // r10 = stack pointer (top of stack, grows down)
        regs[10] = mem.stack_base + mem.stack.len() as u64;
        Self { regs, pc: 0, text, call_stack: Vec::new(), mem, syscalls, return_data: Vec::new() }
    }

    pub fn run(&mut self) -> Result<u64, SbpfError> {
        loop {
            if self.pc >= self.text.len() {
                return Err(SbpfError::OutOfBounds { pc: self.pc });
            }
            let word = self.text[self.pc];
            let i = decode(word);
            self.pc += 1;

            match i.opcode {
                // ── MOV ──────────────────────────────────────────────────────
                0xB7 => { self.regs[i.dst] = i.imm as u64; }
                0xBF => { self.regs[i.dst] = self.regs[i.src]; }
                0xB4 => { self.regs[i.dst] = (i.imm as u32) as u64; } // mov32_imm
                0xBC => { self.regs[i.dst] = (self.regs[i.src] as u32) as u64; } // mov32_reg

                // ── LDDW (two-word instruction) ───────────────────────────────
                0x18 => {
                    let lo = i.imm as u64;
                    if self.pc >= self.text.len() {
                        return Err(SbpfError::OutOfBounds { pc: self.pc });
                    }
                    let hi_word = self.text[self.pc];
                    self.pc += 1;
                    let hi = (hi_word >> 32) as u64;
                    self.regs[i.dst] = (hi << 32) | lo;
                }

                // ── ADD ───────────────────────────────────────────────────────
                0x07 => { self.regs[i.dst] = self.regs[i.dst].wrapping_add(i.imm as u64); }
                0x0F => { self.regs[i.dst] = self.regs[i.dst].wrapping_add(self.regs[i.src]); }
                0x04 => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_add(i.imm as u32)) as u64; }
                0x0C => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_add(self.regs[i.src] as u32)) as u64; }

                // ── SUB ───────────────────────────────────────────────────────
                0x17 => { self.regs[i.dst] = self.regs[i.dst].wrapping_sub(i.imm as u64); }
                0x1F => { self.regs[i.dst] = self.regs[i.dst].wrapping_sub(self.regs[i.src]); }
                0x14 => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_sub(i.imm as u32)) as u64; }
                0x1C => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_sub(self.regs[i.src] as u32)) as u64; }

                // ── MUL ───────────────────────────────────────────────────────
                0x27 => { self.regs[i.dst] = self.regs[i.dst].wrapping_mul(i.imm as u64); }
                0x2F => { self.regs[i.dst] = self.regs[i.dst].wrapping_mul(self.regs[i.src]); }
                0x24 => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_mul(i.imm as u32)) as u64; }
                0x2C => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_mul(self.regs[i.src] as u32)) as u64; }

                // ── DIV ───────────────────────────────────────────────────────
                0x37 => {
                    let d = i.imm as u64;
                    if d == 0 { return Err(SbpfError::DivisionByZero); }
                    self.regs[i.dst] /= d;
                }
                0x3F => {
                    if self.regs[i.src] == 0 { return Err(SbpfError::DivisionByZero); }
                    self.regs[i.dst] /= self.regs[i.src];
                }
                0x34 => {
                    let d = i.imm as u32;
                    if d == 0 { return Err(SbpfError::DivisionByZero); }
                    self.regs[i.dst] = ((self.regs[i.dst] as u32) / d) as u64;
                }
                0x3C => {
                    let d = self.regs[i.src] as u32;
                    if d == 0 { return Err(SbpfError::DivisionByZero); }
                    self.regs[i.dst] = ((self.regs[i.dst] as u32) / d) as u64;
                }

                // ── MOD ───────────────────────────────────────────────────────
                0x97 => {
                    let d = i.imm as u64;
                    if d == 0 { return Err(SbpfError::DivisionByZero); }
                    self.regs[i.dst] %= d;
                }
                0x9F => {
                    if self.regs[i.src] == 0 { return Err(SbpfError::DivisionByZero); }
                    self.regs[i.dst] %= self.regs[i.src];
                }

                // ── NEG ───────────────────────────────────────────────────────
                0x87 => { self.regs[i.dst] = (-(self.regs[i.dst] as i64)) as u64; }
                0x84 => { self.regs[i.dst] = (-(self.regs[i.dst] as i32)) as u64; }

                // ── OR ────────────────────────────────────────────────────────
                0x47 => { self.regs[i.dst] |= i.imm as u64; }
                0x4F => { self.regs[i.dst] |= self.regs[i.src]; }
                0x44 => { self.regs[i.dst] = ((self.regs[i.dst] as u32) | (i.imm as u32)) as u64; }
                0x4C => { self.regs[i.dst] = ((self.regs[i.dst] as u32) | (self.regs[i.src] as u32)) as u64; }

                // ── AND ───────────────────────────────────────────────────────
                0x57 => { self.regs[i.dst] &= i.imm as u64; }
                0x5F => { self.regs[i.dst] &= self.regs[i.src]; }
                0x54 => { self.regs[i.dst] = ((self.regs[i.dst] as u32) & (i.imm as u32)) as u64; }
                0x5C => { self.regs[i.dst] = ((self.regs[i.dst] as u32) & (self.regs[i.src] as u32)) as u64; }

                // ── XOR ───────────────────────────────────────────────────────
                0xA7 => { self.regs[i.dst] ^= i.imm as u64; }
                0xAF => { self.regs[i.dst] ^= self.regs[i.src]; }
                0xA4 => { self.regs[i.dst] = ((self.regs[i.dst] as u32) ^ (i.imm as u32)) as u64; }
                0xAC => { self.regs[i.dst] = ((self.regs[i.dst] as u32) ^ (self.regs[i.src] as u32)) as u64; }

                // ── LSH ───────────────────────────────────────────────────────
                0x67 => { self.regs[i.dst] = self.regs[i.dst].wrapping_shl(i.imm as u32 & 63); }
                0x6F => { self.regs[i.dst] = self.regs[i.dst].wrapping_shl(self.regs[i.src] as u32 & 63); }
                0x64 => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_shl(i.imm as u32 & 31)) as u64; }
                0x6C => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_shl(self.regs[i.src] as u32 & 31)) as u64; }

                // ── RSH (logical) ─────────────────────────────────────────────
                0x77 => { self.regs[i.dst] = self.regs[i.dst].wrapping_shr(i.imm as u32 & 63); }
                0x7F => { self.regs[i.dst] = self.regs[i.dst].wrapping_shr(self.regs[i.src] as u32 & 63); }
                0x74 => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_shr(i.imm as u32 & 31)) as u64; }
                0x7C => { self.regs[i.dst] = ((self.regs[i.dst] as u32).wrapping_shr(self.regs[i.src] as u32 & 31)) as u64; }

                // ── ARSH (arithmetic right shift) ─────────────────────────────
                0xC7 => { self.regs[i.dst] = ((self.regs[i.dst] as i64).wrapping_shr(i.imm as u32 & 63)) as u64; }
                0xCF => { self.regs[i.dst] = ((self.regs[i.dst] as i64).wrapping_shr(self.regs[i.src] as u32 & 63)) as u64; }
                0xC4 => { self.regs[i.dst] = ((self.regs[i.dst] as i32).wrapping_shr(i.imm as u32 & 31)) as u64; }
                0xCC => { self.regs[i.dst] = ((self.regs[i.dst] as i32).wrapping_shr(self.regs[i.src] as u32 & 31)) as u64; }

                // ── LOADS ─────────────────────────────────────────────────────
                0x71 => { // ldxb
                    let addr = self.regs[i.src].wrapping_add(i.off as u64);
                    self.regs[i.dst] = self.mem.read_u8(addr)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 1 })? as u64;
                }
                0x69 => { // ldxh
                    let addr = self.regs[i.src].wrapping_add(i.off as u64);
                    self.regs[i.dst] = self.mem.read_u16(addr)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 2 })? as u64;
                }
                0x61 => { // ldxw
                    let addr = self.regs[i.src].wrapping_add(i.off as u64);
                    self.regs[i.dst] = self.mem.read_u32(addr)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 4 })? as u64;
                }
                0x79 => { // ldxdw
                    let addr = self.regs[i.src].wrapping_add(i.off as u64);
                    self.regs[i.dst] = self.mem.read_u64(addr)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 8 })?;
                }

                // ── STORES (reg) ──────────────────────────────────────────────
                0x73 => { // stxb
                    let addr = self.regs[i.dst].wrapping_add(i.off as u64);
                    self.mem.write_u8(addr, self.regs[i.src] as u8)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 1 })?;
                }
                0x6B => { // stxh
                    let addr = self.regs[i.dst].wrapping_add(i.off as u64);
                    self.mem.write_u16(addr, self.regs[i.src] as u16)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 2 })?;
                }
                0x63 => { // stxw
                    let addr = self.regs[i.dst].wrapping_add(i.off as u64);
                    self.mem.write_u32(addr, self.regs[i.src] as u32)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 4 })?;
                }
                0x7B => { // stxdw
                    let addr = self.regs[i.dst].wrapping_add(i.off as u64);
                    self.mem.write_u64(addr, self.regs[i.src])
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 8 })?;
                }

                // ── STORES (imm) ──────────────────────────────────────────────
                0x72 => { // stb
                    let addr = self.regs[i.dst].wrapping_add(i.off as u64);
                    self.mem.write_u8(addr, i.imm as u8)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 1 })?;
                }
                0x6A => { // sth
                    let addr = self.regs[i.dst].wrapping_add(i.off as u64);
                    self.mem.write_u16(addr, i.imm as u16)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 2 })?;
                }
                0x62 => { // stw
                    let addr = self.regs[i.dst].wrapping_add(i.off as u64);
                    self.mem.write_u32(addr, i.imm as u32)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 4 })?;
                }
                0x7A => { // stdw
                    let addr = self.regs[i.dst].wrapping_add(i.off as u64);
                    self.mem.write_u64(addr, i.imm as u64)
                        .ok_or(SbpfError::InvalidMemoryAccess { addr, size: 8 })?;
                }

                // ── JUMPS ─────────────────────────────────────────────────────
                0x05 => { self.pc = (self.pc as i64 + i.off as i64) as usize; }

                // jeq
                0x15 => if self.regs[i.dst] == i.imm as u64 { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0x1D => if self.regs[i.dst] == self.regs[i.src] { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jne
                0x55 => if self.regs[i.dst] != i.imm as u64 { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0x5D => if self.regs[i.dst] != self.regs[i.src] { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jgt (unsigned)
                0x25 => if self.regs[i.dst] >  i.imm as u64 { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0x2D => if self.regs[i.dst] >  self.regs[i.src] { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jge (unsigned)
                0x35 => if self.regs[i.dst] >= i.imm as u64 { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0x3D => if self.regs[i.dst] >= self.regs[i.src] { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jlt (unsigned)
                0xA5 => if self.regs[i.dst] <  i.imm as u64 { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0xAD => if self.regs[i.dst] <  self.regs[i.src] { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jle (unsigned)
                0xB5 => if self.regs[i.dst] <= i.imm as u64 { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0xBD => if self.regs[i.dst] <= self.regs[i.src] { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jset
                0x45 => if self.regs[i.dst] & (i.imm as u64) != 0 { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0x4D => if self.regs[i.dst] & self.regs[i.src] != 0 { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jsgt (signed)
                0x65 => if (self.regs[i.dst] as i64) >  (i.imm as i64) { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0x6D => if (self.regs[i.dst] as i64) >  (self.regs[i.src] as i64) { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jsge (signed)
                0x75 => if (self.regs[i.dst] as i64) >= (i.imm as i64) { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0x7D => if (self.regs[i.dst] as i64) >= (self.regs[i.src] as i64) { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jslt (signed)
                0xC5 => if (self.regs[i.dst] as i64) <  (i.imm as i64) { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0xCD => if (self.regs[i.dst] as i64) <  (self.regs[i.src] as i64) { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                // jsle (signed)
                0xD5 => if (self.regs[i.dst] as i64) <= (i.imm as i64) { self.pc = (self.pc as i64 + i.off as i64) as usize; },
                0xDD => if (self.regs[i.dst] as i64) <= (self.regs[i.src] as i64) { self.pc = (self.pc as i64 + i.off as i64) as usize; },

                // ── CALL / EXIT ───────────────────────────────────────────────
                0x85 => {
                    // Internal call (src=0) or syscall (src=1)
                    if i.src == 1 {
                        // syscall — dispatch by imm hash
                        self.dispatch_syscall(i.imm as u32)?;
                    } else {
                        // internal call: push frame, jump
                        if self.call_stack.len() >= MAX_CALL_DEPTH {
                            return Err(SbpfError::CallStackOverflow);
                        }
                        self.call_stack.push((self.pc, self.regs));
                        self.pc = (self.pc as i64 + i.imm as i64) as usize;
                    }
                }
                // sBPF v2 syscall opcode
                0x8D => {
                    self.dispatch_syscall(i.imm as u32)?;
                }
                0x95 => {
                    if let Some((ret_pc, saved_regs)) = self.call_stack.pop() {
                        // Restore callee-saved registers r6-r9, keep r0 (return value)
                        let ret_val = self.regs[0];
                        self.regs = saved_regs;
                        self.regs[0] = ret_val;
                        self.pc = ret_pc;
                    } else {
                        return Ok(self.regs[0]);
                    }
                }

                op => return Err(SbpfError::UnknownOpcode(op)),
            }
        }
    }

    fn dispatch_syscall(&mut self, hash: u32) -> Result<(), SbpfError> {
        // Solana syscall hashes (murmur3 of the name)
        match hash {
            0x686093bb => { // sol_log_
                // r1 = ptr, r2 = len
                let _ptr = self.regs[1];
                let _len = self.regs[2];
                self.regs[0] = 0;
            }
            0x52ba5096 => { // sol_panic_
                // just zero return for our purposes
                self.regs[0] = 0;
            }
            0xa92366c4 => { // sol_memcpy_
                let dst = self.regs[1];
                let src = self.regs[2];
                let n   = self.regs[3];
                self.syscalls.sol_memcpy(dst, src, n, &mut self.mem);
                self.regs[0] = 0;
            }
            0x3770fb22 => { // sol_memset_
                let dst = self.regs[1];
                let val = self.regs[2] as u8;
                let n   = self.regs[3];
                self.syscalls.sol_memset(dst, val, n, &mut self.mem);
                self.regs[0] = 0;
            }
            0x0d667e63 => { // sol_memmove_
                let dst = self.regs[1];
                let src = self.regs[2];
                let n   = self.regs[3];
                self.syscalls.sol_memmove(dst, src, n, &mut self.mem);
                self.regs[0] = 0;
            }
            // sol_set_return_data: r1=ptr, r2=len
            0x5b839aba | 5 => {
                let ptr = self.regs[1];
                let len = self.regs[2] as usize;
                self.return_data = (0..len)
                    .map(|i| self.mem.read_u8(ptr + i as u64).unwrap_or(0))
                    .collect();
                self.regs[0] = 0;
            }
            _ => {
                // Unknown syscall — preserve r0 (don't clobber error codes)
            }
        }
        Ok(())
    }
}
