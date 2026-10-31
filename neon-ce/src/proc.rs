//! Process access for the Windows target: regions, modules, guarded reads, safe writes.

use std::collections::HashMap;
use std::ffi::c_void;

use anyhow::{bail, Context, Result};
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Memory::{
    VirtualProtectEx, VirtualQueryEx, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ,
    PAGE_EXECUTE_READWRITE, PAGE_EXECUTE_WRITECOPY, PAGE_GUARD, PAGE_NOACCESS, PAGE_READONLY,
    PAGE_READWRITE, PAGE_WRITECOPY,
};
use windows::Win32::System::ProcessStatus::{
    EnumProcessModulesEx, GetModuleBaseNameW, GetModuleFileNameExW, GetModuleInformation,
    LIST_MODULES_ALL, MODULEINFO,
};
use windows::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_ACCESS_RIGHTS,
    PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
    QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
};
use windows::Win32::Foundation::{STILL_ACTIVE, ERROR_PARTIAL_COPY};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};

pub const MAX_REGION: u64 = 64 << 20;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    Private,
    Mapped,
    Image,
}

#[derive(Copy, Clone, Debug)]
pub struct Region {
    pub base: u64,
    pub size: u64,
    pub protect: u32,
    pub kind: Kind,
    pub writable: bool,
}

#[derive(Clone, Debug)]
pub struct Module {
    pub name: String,
    pub base: u64,
    pub size: u64,
}

pub struct Process {
    handle: HANDLE,
    pub pid: u32,
    pub exe: String,
    /// Pages whose protection we widened to write, with the original value.
    widened: HashMap<u64, u32>,
    pub modules: Vec<Module>,
}

fn rights() -> PROCESS_ACCESS_RIGHTS {
    PROCESS_QUERY_INFORMATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_VM_OPERATION
}

impl Process {
    pub fn open(pid: u32) -> Result<Process> {
        unsafe {
            let handle = OpenProcess(rights(), false, pid)
                .with_context(|| format!("OpenProcess({pid}) - run elevated?"))?;
            let mut p = Process {
                handle,
                pid,
                exe: String::new(),
                widened: HashMap::new(),
                modules: Vec::new(),
            };
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            if QueryFullProcessImageNameW(
                handle,
                PROCESS_NAME_FORMAT(0),
                PWSTR(buf.as_mut_ptr()),
                &mut len,
            )
            .is_ok()
            {
                p.exe = String::from_utf16_lossy(&buf[..len as usize]);
            }
            if let Ok(mods) = p.module_list() {
                p.modules = mods;
            }
            Ok(p)
        }
    }

    pub fn handle(&self) -> HANDLE {
        self.handle
    }

    pub fn alive(&self) -> bool {
        let mut code = 0u32;
        unsafe { GetExitCodeProcess(self.handle, &mut code).is_ok() && code as i32 == STILL_ACTIVE.0 }
    }

    fn module_list(&self) -> Result<Vec<Module>> {
        unsafe {
            let mut mods: [windows::Win32::Foundation::HMODULE; 1024] = [Default::default(); 1024];
            let mut needed = 0u32;
            EnumProcessModulesEx(
                self.handle,
                mods.as_mut_ptr(),
                std::mem::size_of_val(&mods) as u32,
                &mut needed,
                LIST_MODULES_ALL,
            )
            .context("EnumProcessModulesEx")?;
            let count = (needed as usize / std::mem::size_of::<isize>()).min(mods.len());
            let mut out = Vec::with_capacity(count);
            for m in mods.iter().take(count) {
                let mut info = MODULEINFO::default();
                if GetModuleInformation(
                    self.handle,
                    *m,
                    &mut info,
                    std::mem::size_of::<MODULEINFO>() as u32,
                )
                .is_ok()
                {
                    let mut name = [0u16; 260];
                    let n = GetModuleBaseNameW(self.handle, Some(*m), &mut name);
                    out.push(Module {
                        name: String::from_utf16_lossy(&name[..n as usize]),
                        base: info.lpBaseOfDll as usize as u64,
                        size: info.SizeOfImage as u64,
                    });
                }
            }
            Ok(out)
        }
    }

    pub fn module_by_name(&self, want: &str) -> Option<&Module> {
        let want = want.to_ascii_lowercase();
        self.modules
            .iter()
            .find(|m| m.name.to_ascii_lowercase() == want)
            .or_else(|| {
                self.modules
                    .iter()
                    .find(|m| m.name.to_ascii_lowercase().contains(&want))
            })
    }

    /// Image base of the main executable (the first module of the process).
    pub fn main_base(&self) -> u64 {
        self.entity_base_of_exe()
    }

    fn entity_base_of_exe(&self) -> u64 {
        self.modules
            .iter()
            .find(|m| self.exe.to_ascii_lowercase().ends_with(&m.name.to_ascii_lowercase()))
            .map(|m| m.base)
            .unwrap_or(0)
    }

    /// Every committed region between `lo` and `hi`, in address order.
    pub fn regions(&self, lo: u64, hi: u64, include_guarded: bool) -> Vec<Region> {
        let mut out = Vec::new();
        let mut addr = lo.max(0x10000);
        while addr < hi {
            let mut mbi = MEMORY_BASIC_INFORMATION::default();
            let got = unsafe {
                VirtualQueryEx(
                    self.handle,
                    Some(addr as *const c_void),
                    &mut mbi,
                    std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                )
            };
            if got != std::mem::size_of::<MEMORY_BASIC_INFORMATION>() {
                // holes and the 64-bit gap: step and keep walking
                addr += 0x1000;
                continue;
            }
            let base = mbi.BaseAddress as usize as u64;
            let size = mbi.RegionSize as u64;
            if size == 0 {
                break;
            }
            let next = base.saturating_add(size);
            let p = mbi.Protect.0;
            let readable = p & PAGE_NOACCESS.0 == 0 && (include_guarded || p & PAGE_GUARD.0 == 0);
            if mbi.State == windows::Win32::System::Memory::MEM_COMMIT && readable {
                let kind = if mbi.Type == windows::Win32::System::Memory::MEM_IMAGE {
                    Kind::Image
                } else if mbi.Type == windows::Win32::System::Memory::MEM_MAPPED {
                    Kind::Mapped
                } else {
                    Kind::Private
                };
                let prot = p & 0xFF;
                let writable = prot == PAGE_READWRITE.0
                    || prot == PAGE_WRITECOPY.0
                    || prot == PAGE_EXECUTE_READWRITE.0
                    || prot == PAGE_EXECUTE_WRITECOPY.0;
                out.push(Region {
                    base,
                    size,
                    protect: p,
                    kind,
                    writable,
                });
            }
            if next <= addr {
                addr += 0x1000;
            } else {
                addr = next;
            }
        }
        out
    }

    /// Whole address space of the process (32-bit and 64-bit user ranges).
    pub fn all_regions(&self, include_guarded: bool) -> Vec<Region> {
        self.regions(0x10000, 0x0000_7FFF_FFFF_FFFF, include_guarded)
    }

    /// Read into `buf`, halving on failure so a guard page in the span does not lose
    /// the whole chunk. Bytes that cannot be read are left untouched.
    pub fn read_into(&self, addr: u64, buf: &mut [u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }
        let mut got = 0usize;
        let ok = unsafe {
            ReadProcessMemory(
                self.handle,
                addr as *const c_void,
                buf.as_mut_ptr() as *mut c_void,
                buf.len(),
                Some(&mut got),
            )
        };
        if ok.is_ok() && got == buf.len() {
            return got;
        }
        if ok.is_err() && got == 0 && buf.len() <= 0x1000 {
            return 0;
        }
        let half = (buf.len() / 2) & !3;
        if half == 0 {
            return got;
        }
        let a = self.read_into(addr, &mut buf[..half]);
        let b = self.read_into(addr + half as u64, &mut buf[half..]);
        a + b
    }

    pub fn read(&self, addr: u64, len: usize) -> Option<Vec<u8>> {
        let mut buf = vec![0u8; len];
        (self.read_into(addr, &mut buf) == len).then_some(buf)
    }

    /// Read a u64 pointer if it lands in the user range.
    pub fn read_ptr(&self, addr: u64) -> Option<u64> {
        let b = self.read(addr, 8)?;
        Some(u64::from_le_bytes(b[..8].try_into().ok()?))
    }

    /// Make `addr` writable for the lifetime of the process object.
    pub fn make_writable(&mut self, addr: u64) -> Result<()> {
        let page = addr & !0xFFF;
        if self.widened.contains_key(&page) {
            return Ok(());
        }
        let regions = self.regions(page, page + 0x1000, true);
        let Some(r) = regions.first() else {
            bail!("{addr:#x} is not committed");
        };
        if r.writable {
            return Ok(());
        }
        let mut old = PAGE_READONLY;
        unsafe {
            VirtualProtectEx(
                self.handle,
                page as *const c_void,
                (addr as usize - page as usize).max(4) + 4,
                PAGE_READWRITE,
                &mut old,
            )
            .with_context(|| format!("VirtualProtectEx({addr:#x})"))?;
        }
        self.widened.insert(page, old.0);
        Ok(())
    }

    pub fn write(&mut self, addr: u64, bytes: &[u8]) -> Result<()> {
        self.make_writable(addr)?;
        let mut got = 0usize;
        unsafe {
            WriteProcessMemory(
                self.handle,
                addr as *const c_void,
                bytes.as_ptr() as *const c_void,
                bytes.len(),
                Some(&mut got),
            )
            .with_context(|| format!("WriteProcessMemory({addr:#x}, {} bytes)", bytes.len()))?;
        }
        if got != bytes.len() {
            bail!("short write at {addr:#x}: {got}/{}", bytes.len());
        }
        Ok(())
    }

    /// Restore protections widened by `write`.
    pub fn close(&mut self) {
        for (page, old) in self.widened.drain() {
            let mut dummy = PAGE_READONLY;
            unsafe {
                let _ = VirtualProtectEx(
                    self.handle,
                    page as *const c_void,
                    0x1000,
                    windows::Win32::System::Memory::PAGE_PROTECTION_FLAGS(old),
                    &mut dummy,
                );
            }
        }
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

unsafe impl Send for Process {}
unsafe impl Sync for Process {}

impl Drop for Process {
    fn drop(&mut self) {
        self.close();
    }
}

/// True when the current process token is elevated.
pub fn is_elevated() -> bool {
    use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

/// `ERROR_PARTIAL_COPY` numeric value, kept for callers that log it.
pub const PARTIAL_COPY: u32 = ERROR_PARTIAL_COPY.0;

#[allow(dead_code)]
fn _unused(_: &[u8]) -> BOOL {
    BOOL(0)
}
impl crate::scan::Memory for Process {
    fn pid(&self) -> u32 {
        self.pid
    }

    fn scan_regions(&self, scope: &crate::region::Scope) -> Vec<(u64, u64)> {
        use crate::region::{Scope, HEAP_MIN};
        let allowed = |r: &Region| match scope {
            Scope::All => true,
            Scope::Private => r.kind == Kind::Private,
            Scope::Mapped => r.kind == Kind::Mapped,
            Scope::Image => r.kind == Kind::Image,
            Scope::Heap => r.kind == Kind::Private && r.writable && r.size >= HEAP_MIN,
            Scope::Module(_) => true,
            Scope::Range(b, s) => r.base < b + s && r.base + r.size > *b,
        };
        let window = match scope {
            Scope::Module(name) => self
                .module_by_name(name)
                .map(|m| (m.base, m.base + m.size))
                .unwrap_or((1, 0)),
            Scope::Range(b, s) => (*b, b.saturating_add(*s)),
            _ => (0x10000, 0x0000_7FFF_FFFF_FFFF),
        };
        let mut out = Vec::new();
        for r in self.regions(window.0, window.1, false) {
            if !allowed(&r) {
                continue;
            }
            let mut base = r.base.max(window.0);
            let end = (r.base + r.size).min(window.1);
            while base < end {
                let len = (end - base).min(MAX_REGION);
                out.push((base, len));
                base += len;
            }
        }
        out
    }

    fn read_into(&self, addr: u64, buf: &mut [u8]) -> usize {
        Process::read_into(self, addr, buf)
    }
}
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};

/// Pid of the first running MapleStory.exe.
pub fn find_game_pid() -> Option<u32> {
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).ok()?;
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let found = if Process32FirstW(snap, &mut entry).is_err() {
            None
        } else {
            let mut hit = None;
            loop {
                let end = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(0);
                let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                if name.eq_ignore_ascii_case("MapleStory.exe") {
                    hit = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snap, &mut entry).is_err() {
                    break;
                }
            }
            hit
        };
        let _ = CloseHandle(snap);
        found
    }
}
