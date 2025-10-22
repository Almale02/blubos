use core::fmt::Write;

use alloc::boxed::Box;
use spin::Once;

#[inline]
pub unsafe fn outl(port: u16, val: u32) {
    unsafe {
        core::arch::asm!("out dx, eax", in("dx") port, in("eax") val, options(nostack, preserves_flags));
    }
}
#[inline]
pub unsafe fn inl(port: u16) -> u32 {
    let v: u32;
    unsafe {
        core::arch::asm!("in eax, dx", in("dx") port, out("eax") v, options(nostack, preserves_flags));
    }
    v
}

const PCI_CONFIG_ADDRESS: u16 = 0xCF8;
const PCI_CONFIG_DATA: u16 = 0xCFC;

#[inline]
pub fn make_address(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    // bit 31 enable, bits: bus[23:16], device[15:11], function[10:8], reg[7:2], align 4
    (1u32 << 31)
        | ((bus as u32) << 16)
        | ((dev as u32) << 11)
        | ((func as u32) << 8)
        | ((offset as u32) & 0xFC)
}

#[inline]
pub unsafe fn cfg_read_u32(bus: u8, dev: u8, func: u8, offset: u8) -> u32 {
    let addr = make_address(bus, dev, func, offset);
    unsafe {
        outl(PCI_CONFIG_ADDRESS, addr);
        inl(PCI_CONFIG_DATA)
    }
}

#[inline]
pub unsafe fn cfg_write_u32(bus: u8, dev: u8, func: u8, offset: u8, value: u32) {
    let addr = make_address(bus, dev, func, offset);
    unsafe {
        outl(PCI_CONFIG_ADDRESS, addr);
        outl(PCI_CONFIG_DATA, value);
    }
}

#[inline]
pub unsafe fn cfg_read_u16(bus: u8, dev: u8, func: u8, offset: u8) -> u16 {
    let v = unsafe { cfg_read_u32(bus, dev, func, offset & !2) };
    if (offset & 2) == 0 {
        (v & 0xFFFF) as u16
    } else {
        (v >> 16) as u16
    }
}

#[inline]
pub unsafe fn cfg_read_u8(bus: u8, dev: u8, func: u8, offset: u8) -> u8 {
    let v = unsafe { cfg_read_u32(bus, dev, func, offset & !3) };
    ((v >> ((offset & 3) * 8)) & 0xFF) as u8
}

#[derive(Debug, Clone, Copy)]
pub struct PciId {
    pub vendor: u16,
    pub device: u16,
}

#[derive(Debug, Clone, Copy)]
pub struct PciClass {
    pub class: u8,
    pub subclass: u8,
    pub prog_if: u8,
    pub revision: u8,
}

#[derive(Debug, Clone, Copy)]
pub struct PciLocation {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

#[derive(Debug)]
pub struct PciFunc {
    pub loc: PciLocation,
    pub id: PciId,
    pub class: PciClass,
    pub header_type: u8,
    pub bars: [Option<u64>; 6], // 64-bit supported (BAR pairs)
    pub irq_line: u8,
    pub irq_pin: u8,
    pub secondary_bus: Option<u8>, // if bridge
}
impl PciFunc {
    pub fn dump_func(&self, mut writer: impl Write) {
        let _ = writeln!(
            &mut writer,
            "{:02x}:{:02x}.{} vid:did={:04x}:{:04x} class={:02x}:{:02x}:{:02x} rev={:02x} hdr={:02x} irq(l/p)={}/{}",
            self.loc.bus,
            self.loc.device,
            self.loc.function,
            self.id.vendor,
            self.id.device,
            self.class.class,
            self.class.subclass,
            self.class.prog_if,
            self.class.revision,
            self.header_type,
            self.irq_line,
            self.irq_pin,
        );
        for (i, bar) in self.bars.iter().enumerate() {
            if let Some(base) = bar {
                let _ = writeln!(&mut writer, "  BAR{} = 0x{:016x}", i, base);
            }
        }

        // Highlight USB controllers
        if self.class.class == 0x0C && self.class.subclass == 0x03 {
            let kind = match self.class.prog_if {
                0x00 => "UHCI",
                0x10 => "OHCI",
                0x20 => "EHCI",
                0x30 => "xHCI",
                _ => "USB(unknown)",
            };
            let _ = writeln!(&mut writer, "  -> USB controller detected: {}", kind);
        }
        if let Some(sb) = self.secondary_bus {
            let _ = writeln!(&mut writer, "  (PCI-PCI bridge) secondary bus: {}", sb);
        }
    }
}

fn is_multi_function(header_type: u8) -> bool {
    (header_type & 0x80) != 0
}

fn is_bridge(header_type: u8) -> bool {
    (header_type & 0x7F) == 0x01
}

unsafe fn read_bars(bus: u8, dev: u8, func: u8, header_type: u8) -> [Option<u64>; 6] {
    // Type 0 header has 6 BARs, Type 1 (bridge) has 2 BARs
    let bar_count = if (header_type & 0x7F) == 0x00 { 6 } else { 2 };
    let mut bars: [Option<u64>; 6] = [None; 6];
    let mut i = 0;
    while i < bar_count {
        let off = 0x10 + (i as u8) * 4;
        let bar_lo = unsafe { cfg_read_u32(bus, dev, func, off) };
        if bar_lo == 0 {
            bars[i] = None;
            i += 1;
            continue;
        }
        if (bar_lo & 1) != 0 {
            // I/O space BAR (rare for EHCI/xHCI)
            let base = (bar_lo & 0xFFFFFFFC) as u64;
            bars[i] = Some(base);
            i += 1;
        } else {
            // Memory BAR
            let typ = (bar_lo >> 1) & 0x3;
            if typ == 0x2 && i + 1 < bar_count {
                // 64-bit BAR: next BAR is high dword
                let bar_hi = unsafe { cfg_read_u32(bus, dev, func, off + 4) } as u64;
                let base = (bar_hi << 32) | (bar_lo as u64 & 0xFFFF_FFF0);
                bars[i] = Some(base);
                // skip the high part BAR
                i += 2;
            } else {
                let base = (bar_lo & 0xFFFF_FFF0) as u64;
                bars[i] = Some(base);
                i += 1;
            }
        }
    }
    bars
}

unsafe fn read_pci_function(bus: u8, dev: u8, func: u8) -> Option<PciFunc> {
    unsafe {
        let vendor = cfg_read_u16(bus, dev, func, 0x00);
        if vendor == 0xFFFF {
            return None;
        }
        let device = cfg_read_u16(bus, dev, func, 0x02);

        let rev = cfg_read_u8(bus, dev, func, 0x08);
        let prog_if = cfg_read_u8(bus, dev, func, 0x09);
        let subclass = cfg_read_u8(bus, dev, func, 0x0A);
        let class = cfg_read_u8(bus, dev, func, 0x0B);
        let header_type = cfg_read_u8(bus, dev, func, 0x0E);

        let bars = read_bars(bus, dev, func, header_type);
        let irq_line = cfg_read_u8(bus, dev, func, 0x3C);
        let irq_pin = cfg_read_u8(bus, dev, func, 0x3D);

        let secondary_bus = if is_bridge(header_type) {
            Some(cfg_read_u8(bus, dev, func, 0x19)) // secondary bus number
        } else {
            None
        };

        let func = PciFunc {
            loc: PciLocation {
                bus,
                device: dev,
                function: func,
            },
            id: PciId { vendor, device },
            class: PciClass {
                class,
                subclass,
                prog_if,
                revision: rev,
            },
            header_type,
            bars,
            irq_line,
            irq_pin,
            secondary_bus,
        };
        Some(func)
    }
}
pub static mut PCI_SCANNER: Once<PciScanner> = Once::new();
pub struct PciScanner {
    // bus 256, device 32 function 8
    pub buses: Box<[Option<PciBus>; 256]>,
}
pub struct PciBus {
    pub devices: Box<[Option<PciDevice>; 32]>,
}
impl Default for PciBus {
    fn default() -> Self {
        Self {
            devices: Box::new(core::array::from_fn(|_| None)),
        }
    }
}
pub struct PciDevice {
    pub functions: Box<[Option<PciFunc>; 8]>,
}
impl Default for PciDevice {
    fn default() -> Self {
        Self {
            functions: Box::new(core::array::from_fn(|_| None)),
        }
    }
}

impl PciScanner {
    pub fn new() -> Self {
        Self {
            buses: Box::new(core::array::from_fn(|_| None)),
        }
    }

    pub fn add_function(&mut self, func: PciFunc) -> &PciFunc {
        let b = func.loc.bus as usize;
        let d = func.loc.device as usize;
        let f = func.loc.function as usize;

        debug_assert!(b < 256, "PCI bus out of range: {}", b);
        debug_assert!(d < 32, "PCI device out of range: {}", d);
        debug_assert!(f < 8, "PCI function out of range: {}", f);

        // Ensure bus present
        if self.buses[b].is_none() {
            self.buses[b] = Some(PciBus::default());
        }
        let bus = self.buses[b].as_mut().unwrap();

        // Ensure device present
        if bus.devices[d].is_none() {
            bus.devices[d] = Some(PciDevice::default());
        }
        let dev = bus.devices[d].as_mut().unwrap();

        // Place/replace function
        dev.functions[f] = Some(func);
        &dev.functions[f].as_ref().unwrap()
    }

    pub fn scan(&mut self) {
        unsafe {
            for bus in 0u8..=255 {
                for dev in 0u8..32 {
                    // Function 0 first
                    let f0 = match read_pci_function(bus, dev, 0) {
                        Some(f) => self.add_function(f),
                        None => continue,
                    };

                    let multi = is_multi_function(f0.header_type);
                    let mut funcs = 1u8..8;
                    if multi {
                        for func in funcs.by_ref() {
                            match read_pci_function(bus, dev, func) {
                                Some(f) => self.add_function(f),
                                None => continue,
                            };
                        }
                    }
                }
            }
        }
    }
}
pub fn enable_mmio_and_bus_master(loc: PciLocation) {
    unsafe {
        let cmd = cfg_read_u16(loc.bus, loc.device, loc.function, 0x04);
        let cmd = cmd | (1 << 1) | (1 << 2); // Memory Space, Bus Master
        let v = cfg_read_u32(loc.bus, loc.device, loc.function, 0x04 & !2);
        let shift = ((0x04 & 2) as u32) * 8;
        let new = (v & !(0xFFFF << shift)) | ((cmd as u32) << shift);
        cfg_write_u32(loc.bus, loc.device, loc.function, 0x04 & !2, new);
    }
}
pub struct PciFuncIter<'a> {
    buses: &'a [Option<PciBus>; 256],
    b: usize,
    d: usize,
    f: usize,
}

impl<'a> Iterator for PciFuncIter<'a> {
    type Item = &'a PciFunc;

    fn next(&mut self) -> Option<Self::Item> {
        while self.b < 256 {
            if let Some(bus) = &self.buses[self.b] {
                while self.d < 32 {
                    if let Some(dev) = &bus.devices[self.d] {
                        while self.f < 8 {
                            if let Some(func) = &dev.functions[self.f] {
                                // advance f for next call
                                let out = func;
                                self.f += 1;
                                return Some(out);
                            }
                            self.f += 1;
                        }
                        // next device
                        self.f = 0;
                    }
                    self.d += 1;
                }
            }
            // next bus
            self.d = 0;
            self.f = 0;
            self.b += 1;
        }
        None
    }
}

pub struct PciFuncIterMut<'a> {
    // Raw pointer to avoid aliasing problems while iterating mutably.
    buses: *mut [Option<PciBus>; 256],
    b: usize,
    d: usize,
    f: usize,
    _marker: core::marker::PhantomData<&'a mut [Option<PciBus>; 256]>,
}

impl<'a> Iterator for PciFuncIterMut<'a> {
    type Item = &'a mut PciFunc;

    fn next(&mut self) -> Option<Self::Item> {
        // SAFETY:
        // - We only create at most one &mut PciFunc per call.
        // - We advance (b,d,f) BEFORE yielding, so the next call won't touch the same slot.
        // - We never hold intermediate &mut borrows across the yield; we reindex fresh each time.
        unsafe {
            let buses: &mut [Option<PciBus>; 256] = &mut *self.buses;

            while self.b < 256 {
                if buses[self.b].is_none() {
                    self.b += 1;
                    self.d = 0;
                    self.f = 0;
                    continue;
                }
                let bus = buses[self.b].as_mut().unwrap();

                while self.d < 32 {
                    if bus.devices[self.d].is_none() {
                        self.d += 1;
                        self.f = 0;
                        continue;
                    }
                    let dev = bus.devices[self.d].as_mut().unwrap();

                    while self.f < 8 {
                        if let Some(func) = dev.functions[self.f].as_mut() {
                            // Advance indices before returning to avoid aliasing on next call
                            self.f += 1;
                            // Cast lifetime to 'a via raw pointer traversal + PhantomData
                            let func_ptr: *mut PciFunc = func as *mut PciFunc;
                            return Some(&mut *func_ptr);
                        }
                        self.f += 1;
                    }

                    self.d += 1;
                    self.f = 0;
                }

                self.b += 1;
                self.d = 0;
                self.f = 0;
            }

            None
        }
    }
}

impl PciScanner {
    pub fn iter(&self) -> PciFuncIter<'_> {
        PciFuncIter {
            buses: &self.buses,
            b: 0,
            d: 0,
            f: 0,
        }
    }

    pub fn iter_mut(&mut self) -> PciFuncIterMut<'_> {
        PciFuncIterMut {
            buses: (&mut *self.buses) as *mut [Option<PciBus>; 256],
            b: 0,
            d: 0,
            f: 0,
            _marker: core::marker::PhantomData,
        }
    }
}
