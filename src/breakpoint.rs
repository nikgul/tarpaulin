use std::collections::HashMap;
use nix::libc::{pid_t, c_long};
use nix::Result;
use ptrace_control::*;

#[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
const INT: u64 = 0xCC;


/// Breakpoint construct used to monitor program execution. As tarpaulin is an
/// automated process, this will likely have less functionality than most 
/// breakpoint implementations.
#[derive(Debug)]
pub struct Breakpoint { 
    /// Program counter
    pub pc: u64,
    /// Bottom byte of address data. 
    /// This is replaced to enable the interrupt. Rest of data is never changed.
    data: u8,
    /// Reading from memory with ptrace gives addresses aligned to bytes. 
    /// We therefore need to know the shift to place the breakpoint in the right place
    shift: u64,
    /// When execution pauses. 
    is_running: HashMap<pid_t, bool>
}

impl Breakpoint {
    /// Creates a new breakpoint for the given process and program counter. 
    pub fn new(pid:pid_t, pc:u64) ->Result<Breakpoint> {
        let aligned = pc & !0x7u64;
        let data = read_address(pid, aligned)?;
        let shift = 8 * (pc - aligned);
        let data = ((data >> shift) & 0xFF) as u8;
        
        let mut b = Breakpoint{ 
            pc: pc,
            data: data,
            shift: shift,
            is_running: HashMap::new(),
        };
        match b.enable(pid) {
            Ok(_) => Ok(b),
            Err(e) => Err(e)
        }
    }

    /// Attaches the current breakpoint.
    pub fn enable(&mut self, pid: pid_t) -> Result<c_long> {
        let data  = read_address(pid, self.aligned_address())?;
        self.is_running.insert(pid, true);
        let mut intdata = data & (!(0xFFu64 << self.shift) as i64);
        intdata |= (INT << self.shift) as i64;
        write_to_address(pid, self.aligned_address(), intdata)
    }
    
    fn disable(&self, pid: pid_t) -> Result<c_long> {
        // I require the bit fiddlin this end.
        let data = read_address(pid, self.aligned_address())?;
        let mut orgdata = data & (!(0xFFu64 << self.shift) as i64);
        orgdata |= (self.data as i64) << self.shift;
        write_to_address(pid, self.aligned_address(), orgdata)
    }

    /// Processes the breakpoint. This steps over the breakpoint
    pub fn process(&mut self, pid: pid_t) -> Result<bool> {
        let is_running = match self.is_running.get(&pid) {
            Some(r) => *r,
            None => true,
        };
        if is_running {
            self.enable(pid)?;
            self.step(pid)?;
            self.is_running.insert(pid, false);
            Ok(true)
        } else {
            self.disable(pid)?;
            self.enable(pid)?;
            single_step(pid)?;
            self.is_running.insert(pid, true);
            Ok(false)
        }
    }

    /// Steps past the current breakpoint.
    /// For more advanced coverage may interrogate the variables of a branch.
    fn step(&mut self, pid: pid_t) -> Result<c_long> {
        // Remove the breakpoint, reset the program counter to step before it
        // hit the breakpoint then step to execute the original instruction.
        self.disable(pid)?;
        // Need to set the program counter back one.
        set_instruction_pointer(pid, self.pc)?;
        single_step(pid)
    }

    fn aligned_address(&self) -> u64 {
        self.pc & !0x7u64
    }
}
