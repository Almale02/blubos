use crate::pci::usb::xhci::{
    controller::{XHCI_CONTROLLER, XhciController},
    registers::{XhciEventRingDequeuePtr, XhciInterrupterRegister},
    trb::{XhciEmptyTrb, XhciTrbType},
};
use alloc::{boxed::Box, vec::Vec};
use spin::Once;

use crate::{
    allocator::{
        allocator::{AlignedBoxAlloc, GetPhysicalAddr},
        xhci::XhciAllocator,
    },
    byte_ext::ByteExt as _,
    pci::usb::xhci::trb::{XhciLinkTrb, XhciTrb, XhciTrbTag, XhciTrbUnion},
};

pub static mut XHCI_COMMAND_RING: Once<XhciCommandRing> = Once::new();
pub static mut XHCI_EVENT_RING: Once<XhciEventRing> = Once::new();

pub struct XhciCommandRing {
    pub trbs: Box<[XhciTrbUnion; XhciCommandRing::MAX_TRB_COUNT], XhciAllocator>,
    pub trb_tags: [XhciTrbTag; XhciCommandRing::MAX_TRB_COUNT],
    pub staging_trbs: [XhciTrb; XhciCommandRing::MAX_TRB_COUNT],
    pub staging_enqueue_trb_index: usize,
    pub cycle_bit: bool,
    pub trb_physical_base: usize,
}
impl XhciCommandRing {
    const MAX_TRB_COUNT: usize = 64;
    pub fn init_ring() {
        let mut trbs = Box::new_in_aligned(
            [unsafe { core::mem::zeroed() }; Self::MAX_TRB_COUNT],
            XhciAllocator {
                boundary: Some(64.kb()),
            },
            64,
        );
        let mut staging_trbs = [XhciTrb::Empty(XhciEmptyTrb::new_cycle_bit(false)); _];

        let mut link = XhciLinkTrb::new();
        link.set_ring_segment_pointer(trbs.to_physical_addr() as u64);
        link.set_cycle_bit(true);
        link.set_toggle_cycle(true);
        link.set_chain_bit(false);
        link.set_interrupt_on_completion(false);
        link.set_trb_type(XhciTrbType::Link);

        staging_trbs[XhciCommandRing::MAX_TRB_COUNT - 1] = XhciTrb::Link(link);
        trbs[XhciCommandRing::MAX_TRB_COUNT - 1] = XhciTrb::Link(link).to_union().unwrap();
        unsafe {
            XHCI_COMMAND_RING.call_once(|| XhciCommandRing {
                trb_physical_base: trbs.to_physical_addr(),
                trbs: trbs,
                staging_trbs: staging_trbs,
                staging_enqueue_trb_index: 0,
                cycle_bit: true,
                trb_tags: [XhciTrbTag::Empty; _],
            });
        }
    }
    pub fn get() -> &'static XhciCommandRing {
        unsafe { XHCI_COMMAND_RING.get().unwrap() }
    }
    pub fn get_mut() -> &'static mut XhciCommandRing {
        unsafe { XHCI_COMMAND_RING.get_mut().unwrap() }
    }
    pub fn add_trb(&mut self, mut trb: XhciTrb) -> Option<()> {
        if self.staging_enqueue_trb_index == XhciCommandRing::MAX_TRB_COUNT - 1 {
            return None;
        }
        assert!(trb.to_tag() != XhciTrbTag::Empty);
        trb.set_cycle_bit(self.cycle_bit);
        self.staging_trbs[self.staging_enqueue_trb_index] = trb;
        self.staging_enqueue_trb_index += 1;
        Some(())
    }
    pub fn submit_trbs(&mut self) {
        for (i, trb) in (&self.staging_trbs[0..=self.staging_enqueue_trb_index])
            .iter()
            .enumerate()
        {
            self.trbs[i] = trb.to_union().unwrap();
            self.trb_tags[i] = trb.to_tag();
        }
    }
    pub fn flip_ring(&mut self) {
        let next_state = !self.cycle_bit;
        *self.trbs = [XhciTrbUnion {
            Empty: XhciEmptyTrb::new_cycle_bit(!next_state),
        }; _];
        self.trb_tags = [XhciTrbTag::Empty; _];
        self.staging_trbs = [XhciTrb::Empty(XhciEmptyTrb::new_cycle_bit(!next_state)); _];
        self.staging_enqueue_trb_index = 0;
        self.cycle_bit = next_state;

        let mut link = XhciLinkTrb::new();
        link.set_ring_segment_pointer(self.trb_physical_base as u64);
        link.set_cycle_bit(next_state);
        link.set_toggle_cycle(true);
        link.set_chain_bit(false);
        link.set_interrupt_on_completion(false);
        link.set_trb_type(XhciTrbType::Link);

        self.staging_trbs[XhciCommandRing::MAX_TRB_COUNT - 1] = XhciTrb::Link(link);
        self.trbs[XhciCommandRing::MAX_TRB_COUNT - 1] = XhciTrb::Link(link).to_union().unwrap();
    }
}

#[repr(C, align(64))]
#[derive(Debug, Copy, Clone, Default)]
pub struct XhciErstEntry {
    pub ring_segment_base_address: u64,
    // 16 - 4096
    pub ring_segment_size: u16,
    _rsvd: [u16; 3],
}
pub struct XhciEventRing {
    pub trbs: Box<[XhciTrbUnion; Self::SEGMENT_SIZE], XhciAllocator>,
    pub cycle_bit: bool,
    pub segment_table: Box<[XhciErstEntry; Self::SEGMENT_COUNT], XhciAllocator>,
    pub dequeue_idx: usize,
    pub interrupter_idx: usize,
}
impl XhciEventRing {
    pub const SEGMENT_COUNT: usize = 1;
    pub const SEGMENT_SIZE: usize = 128;

    pub fn init_ring() {
        let trbs = Box::new_in_aligned(
            [unsafe { core::mem::zeroed::<XhciTrbUnion>() }; Self::SEGMENT_SIZE],
            XhciAllocator {
                boundary: Some(64.kb()),
            },
            64,
        );
        let mut segment_table = Box::new_in_aligned(
            [unsafe { core::mem::zeroed() }; Self::SEGMENT_COUNT],
            XhciAllocator {
                boundary: Some(1.kb()),
            },
            64,
        );
        let segment = XhciErstEntry {
            ring_segment_base_address: trbs.to_physical_addr() as u64,
            ring_segment_size: Self::SEGMENT_SIZE as u16,
            _rsvd: unsafe { core::mem::zeroed() },
        };
        segment_table[0] = segment;
        let controller = unsafe { XHCI_CONTROLLER.get_mut().unwrap() };
        controller.runtime_registers.modify(|runtime| {
            let interrupter = &mut runtime.interrupters[0];
            interrupter.erst_size = 1;
            let dequeue_addr = trbs.to_physical_addr();
            interrupter.event_ring_dequeue_ptr =
                XhciEventRingDequeuePtr::from_ptr(dequeue_addr as u64).with_handler_busy(true);
            interrupter.event_ring_segment_table_address = segment_table.to_physical_addr() as u64;
        });

        unsafe {
            XHCI_EVENT_RING.call_once(|| Self {
                trbs,
                cycle_bit: true,
                segment_table: segment_table,
                dequeue_idx: 0,
                interrupter_idx: 0,
            })
        };
    }
    pub fn get_dequeue_ptr(&self) -> XhciEventRingDequeuePtr {
        XhciEventRingDequeuePtr::from_ptr(
            (self.trbs.to_physical_addr() + size_of::<XhciTrbUnion>() * self.dequeue_idx) as u64,
        )
    }
    pub fn dequeue_trb(&mut self) -> Option<XhciTrbUnion> {
        if self.trbs[self.dequeue_idx].cycle_bit() != self.cycle_bit {
            return None;
        }
        let trb = self.trbs[self.dequeue_idx];
        self.dequeue_idx += 1;
        if self.dequeue_idx == Self::SEGMENT_SIZE {
            self.dequeue_idx = 0;
            self.cycle_bit = !self.cycle_bit;
        }

        Some(trb)
    }
    /// returns the amount of events processed which have been enqueued
    pub fn dequeue_events(&mut self, buffer: &mut [XhciTrbUnion]) -> XhciEventRingDequeueResult {
        let mut idx = 0;
        while let Some(trb) = self.dequeue_trb() {
            if idx < buffer.len() {
                buffer[idx] = trb;
            } else {
                let controller = unsafe { XHCI_CONTROLLER.get_mut_unchecked() };
                controller.runtime_registers.modify(|runtime| {
                    runtime.interrupters[self.interrupter_idx].event_ring_dequeue_ptr =
                        self.get_dequeue_ptr().with_handler_busy(true);
                });
                return XhciEventRingDequeueResult::BufferToSmall(idx);
            }
            idx += 1;
        }
        let controller = unsafe { XHCI_CONTROLLER.get_mut_unchecked() };
        controller.runtime_registers.modify(|runtime| {
            runtime.interrupters[self.interrupter_idx].event_ring_dequeue_ptr =
                self.get_dequeue_ptr().with_handler_busy(true);
        });
        return XhciEventRingDequeueResult::Finished(idx);
    }
}
/// Containes the number of processed events
pub enum XhciEventRingDequeueResult {
    Finished(usize),
    BufferToSmall(usize),
}

/*






















*/
