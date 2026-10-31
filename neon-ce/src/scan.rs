//! Scan sessions: address store, predicate passes, undo and result paging.
//!
//! A session is created by a first scan (`Cmp` may be a value comparison or one of the
//! "unknown initial value" predicates) and narrowed by later passes. Every pass keeps
//! one undo step and the value a candidate had at the first scan (`first`) so
//! `sameasfirst` survives arbitrary many rounds.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{bail, Result};

use crate::region::Scope;
use crate::value::{self, Cmp, VType};

/// What the scanner needs from a memory source. The real implementation is in
/// `proc`, tests use a flat buffer.
pub trait Memory {
    fn pid(&self) -> u32;
    /// Region candidates for the scan, already limited by `scope`.
    fn scan_regions(&self, scope: &Scope) -> Vec<(u64, u64)>;
    /// Read into `buf`; returns the number of bytes read.
    fn read_into(&self, addr: u64, buf: &mut [u8]) -> usize;
}

pub const CHUNK: usize = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Building,
    Ready,
}

pub struct Session {
    pub pid: u32,
    pub vtype: VType,
    pub scope: Scope,
    pub alignment: usize,
    pub cmp: Cmp,
    addrs: Vec<u64>,
    prev: Vec<Vec<u8>>,
    first: Vec<Vec<u8>>,
    undo: Option<(Vec<u64>, Vec<Vec<u8>>, Vec<Vec<u8>>)>,
    pub state: State,
    pub passes: u32,
    pub value: Option<Vec<u8>>,
    pub value2: Option<Vec<u8>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<AtomicU64>,
}

impl Session {
    pub fn new(pid: u32, vtype: VType, scope: Scope, alignment: usize, cmp: Cmp) -> Session {
        Session {
            pid,
            vtype,
            scope,
            alignment: alignment.max(1),
            cmp,
            addrs: Vec::new(),
            prev: Vec::new(),
            first: Vec::new(),
            undo: None,
            state: State::Building,
            passes: 0,
            value: None,
            value2: None,
            cancel: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn len(&self) -> usize {
        self.addrs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.addrs.is_empty()
    }

    pub fn progress(&self) -> u64 {
        self.progress.load(Ordering::Relaxed)
    }

    pub fn cancel_handle(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    pub fn reset(&mut self) {
        self.addrs.clear();
        self.prev.clear();
        self.first.clear();
        self.undo = None;
        self.state = State::Building;
        self.passes = 0;
        self.progress.store(0, Ordering::Relaxed);
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.take() {
            Some((a, p, f)) => {
                self.addrs = a;
                self.prev = p;
                self.first = f;
                self.passes = self.passes.saturating_sub(1);
                true
            }
            None => false,
        }
    }

    /// First scan across the scoped regions.
    pub fn first_scan(&mut self, mem: &dyn Memory) -> Result<()> {
        self.reset();
        self.state = State::Building;
        let vlen = self.encoded_len()?;
        let align = self.alignment;
        let mut buf = vec![0u8; CHUNK];
        for (base, size) in mem.scan_regions(&self.scope) {
            let mut off = 0u64;
            while off < size {
                if self.cancel.load(Ordering::Relaxed) {
                    self.state = State::Ready;
                    return Ok(());
                }
                let take = ((size - off) as usize).min(buf.len());
                let got = mem.read_into(base + off, &mut buf[..take]);
                if got == 0 {
                    off += (take as u64).max(align as u64);
                    continue;
                }
                let mut i = 0usize;
                while i + vlen <= got {
                    let at = base + off + i as u64;
                    if at % align as u64 == 0 {
                        let cur = &buf[i..i + vlen];
                        if self.accepts(cur, cur, cur, true) {
                            self.addrs.push(at);
                            self.prev.push(cur.to_vec());
                            self.first.push(cur.to_vec());
                        }
                    }
                    i += align.max(1);
                }
                self.progress.fetch_add(got as u64, Ordering::Relaxed);
                off += got as u64;
                if self.addrs.len() > 40_000_000 {
                    self.state = State::Ready;
                    return Ok(());
                }
            }
            if self.cancel.load(Ordering::Relaxed) {
                break;
            }
        }
        self.state = State::Ready;
        self.passes = 1;
        Ok(())
    }

    /// Re-read every candidate and keep those satisfying the comparison.
    pub fn next_scan(&mut self, mem: &dyn Memory, cmp: Cmp) -> Result<usize> {
        if self.state != State::Ready {
            bail!("first scan still running");
        }
        let vlen = self.encoded_len()?;
        let mut keep_addr = Vec::with_capacity(self.addrs.len());
        let mut keep_prev = Vec::with_capacity(self.addrs.len());
        let mut keep_first = Vec::with_capacity(self.addrs.len());
        let mut cur = vec![0u8; vlen];
        let want = self.value.clone();
        let want2 = self.value2.clone();
        for i in 0..self.addrs.len() {
            if self.cancel.load(Ordering::Relaxed) {
                break;
            }
            let a = self.addrs[i];
            if mem.read_into(a, &mut cur) != vlen {
                continue;
            }
            if value::matches(
                self.vtype,
                cmp,
                &cur,
                &self.prev[i],
                &self.first[i],
                want.as_deref(),
                want2.as_deref(),
            ) {
                keep_addr.push(a);
                keep_prev.push(cur.clone());
                keep_first.push(self.first[i].clone());
            }
        }
        self.undo = Some((self.addrs.clone(), self.prev.clone(), self.first.clone()));
        self.addrs = keep_addr;
        self.prev = keep_prev;
        self.first = keep_first;
        self.passes += 1;
        Ok(self.addrs.len())
    }

    pub fn window(&self, offset: usize, limit: usize) -> Vec<(u64, Vec<u8>)> {
        self.addrs
            .iter()
            .skip(offset)
            .take(limit)
            .zip(self.prev.iter().skip(offset).take(limit))
            .map(|(a, v)| (*a, v.clone()))
            .collect()
    }

    pub fn addr_at(&self, index: usize) -> Option<u64> {
        self.addrs.get(index).copied()
    }

    fn accepts(&self, cur: &[u8], prev: &[u8], first: &[u8], initial: bool) -> bool {
        if initial {
            // No previous value yet: the change-shaped comparisons accept everything.
            if matches!(
                self.cmp,
                Cmp::Changed | Cmp::Unchanged | Cmp::Increased | Cmp::Decreased | Cmp::SameAsFirst
            ) {
                return true;
            }
        }
        value::matches(
            self.vtype,
            self.cmp,
            cur,
            prev,
            first,
            self.value.as_deref(),
            self.value2.as_deref(),
        )
    }

    fn encoded_len(&self) -> Result<usize> {
        let n = value::value_len(self.vtype, self.value.as_deref().unwrap_or(&[]));
        if n == 0 {
            bail!("this value type needs a value to scan for");
        }
        Ok(n)
    }
}

#[cfg(test)]
pub mod fake {
    use super::Memory;
    use crate::region::Scope;

    pub struct Buf {
        pub base: u64,
        pub data: Vec<u8>,
    }

    impl Buf {
        pub fn new(base: u64, data: Vec<u8>) -> Buf {
            Buf { base, data }
        }
    }

    impl Memory for Buf {
        fn pid(&self) -> u32 {
            0
        }
        fn scan_regions(&self, _scope: &Scope) -> Vec<(u64, u64)> {
            vec![(self.base, self.data.len() as u64)]
        }
        fn read_into(&self, addr: u64, buf: &mut [u8]) -> usize {
            if addr < self.base {
                return 0;
            }
            let off = (addr - self.base) as usize;
            if off >= self.data.len() {
                return 0;
            }
            let n = buf.len().min(self.data.len() - off);
            buf[..n].copy_from_slice(&self.data[off..off + n]);
            n
        }
    }


}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::region::Scope;

    fn u32_at(n: u32) -> Vec<u8> {
        n.to_le_bytes().to_vec()
    }

    #[test]
    fn exact_then_decrease_narrows() {
        let mut data = Vec::new();
        for n in [162u32, 7, 162, 99, 162, 3] {
            data.extend_from_slice(&u32_at(n));
        }
        let mem = fake::Buf::new(0x10000, data);
        let mut s = Session::new(1, VType::U32, Scope::All, 4, Cmp::Eq);
        s.value = Some(u32_at(162));
        s.first_scan(&mem).unwrap();
        assert_eq!(s.len(), 3);

        // hit one of them
        let mut d = mem.data.clone();
        d[0..4].copy_from_slice(&u32_at(150));
        let mem2 = fake::Buf::new(0x10000, d);
        assert_eq!(s.next_scan(&mem2, Cmp::Decreased).unwrap(), 1);
        assert_eq!(s.addr_at(0), Some(0x10000));
        assert!(s.undo());
        assert_eq!(s.len(), 3);
    }

    #[test]
    fn unknown_value_changed_works() {
        let mem = fake::Buf::new(0x20000, vec![5, 0, 0, 0, 5, 0, 0, 0]);
        let mut s = Session::new(1, VType::U32, Scope::All, 4, Cmp::Changed);
        s.first_scan(&mem).unwrap();
        assert_eq!(s.len(), 2);
        let mem2 = fake::Buf::new(0x20000, vec![5, 0, 0, 0, 6, 0, 0, 0]);
        assert_eq!(s.next_scan(&mem2, Cmp::Changed).unwrap(), 1);
        assert_eq!(s.addr_at(0), Some(0x20004));
    }

    #[test]
    fn alignment_is_respected() {
        let mem = fake::Buf::new(0x10000, vec![0xAA, 1, 0, 0, 2, 0, 0, 0]);
        let mut s = Session::new(1, VType::U32, Scope::All, 4, Cmp::Between);
        s.value = Some(u32_at(1));
        s.value2 = Some(u32_at(3));
        s.first_scan(&mem).unwrap();
        assert_eq!(s.addr_at(0), Some(0x10004));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn window_pages_results() {
        let mut data = Vec::new();
        for _ in 0..10 {
            data.extend_from_slice(&u32_at(42));
        }
        let mem = fake::Buf::new(0x10000, data);
        let mut s = Session::new(1, VType::U32, Scope::All, 4, Cmp::Eq);
        s.value = Some(u32_at(42));
        s.first_scan(&mem).unwrap();
        let w = s.window(8, 5);
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].0, 0x10020);
    }
}
