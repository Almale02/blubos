use bitflags::bitflags;

bitflags! {
    /// host controller command register
    #[derive(Copy, Clone, Debug)]
    pub struct EhciCmd: u32 {
        /// RW
        const RUN = 1 << 0;
        /// RW
        const RESET = 1 << 1;
        /// RW
        const PERIODIC_SCHEDULE_ENABLE = 1 << 4;
        /// RW
        const ASYNC_SCHEDULE_ENABLE = 1 << 5;
        /// RW
        const ASYNC_ADVANCE_DOORBELL= 1 << 6;

    }
    /// host controller status register, write back to ack
    #[derive(Copy, Clone, Debug)]
    pub struct EhciStatus: u32  {
        /// W1C
        const INTERRUPT = 1 << 0;
        /// W1C
        const ERROR_INTERRUPT = 1 << 1;
        /// W1C
        const PORT_CHANGE = 1 << 2;
        /// W1C
        const FRAME_LIST_ROLLOVER = 1 << 3;
        /// W1C
        const HOST_SYSTEM_ERROR = 1 << 4;
        /// W1C
        const ASYNC_ADVANCE_INTERRUPT = 1 << 5;
        /// RO
        const HALTED = 1 << 12;
        /// RO
        const PERIODIC_STATUS = 1 << 14;
        /// RO
        const ASYNC_STATUS = 1 << 15;

    }
    /// unique per port
    #[derive(Copy, Clone, Debug)]
    pub struct EhciPortControl: u32 {
        /// RO
        const CURR_CONNECT_STATUS = 1 << 0;
        /// W1C
        const CONNECT_STATUS_CHANGE = 1 << 1;
        /// RO enable by reset sequence
        const ENABLED = 1 << 2;
        /// W1C
        const ENABLE_CHANGE = 1 << 3;
        /// RO
        const OVER_CURRENT = 1 << 4;
        /// W1C
        const OVER_CURRENT_CHANGE = 1 << 5;
        /// RW
        const SUSPEND = 1 << 7;
        /// RW to start reset write 1 then wait some time (eg 30ms) then write 0, enabled needs to be 0 when reset is 1
        const RESET = 1 << 8;
        /// RW keep zero for ehci control, in low speed needs to be 1
        const PORT_OWNER = 1 << 13;
    }
    #[derive(Copy, Clone, Debug)]
    pub struct EhciControllerCapability: u32 {
        /// RO if 0, only supports physical addresses below 4GiB
        const ADDRESSING_MODE = 1 << 0;
        /// RO
        const PROGRAMABLE_FRAME_LIST_FLAG= 1 << 1;
    }
    #[derive(Copy, Clone, Debug)]
    pub struct EhciIntr: u32 {
        // RW
        const INTERRUPT_ENABLE = 1 << 0;
        // RW
        const ERROR_INTERRUPT_ENABLE= 1 << 1;
        // RW
        const PORT_CHANGE_ENABLE = 1 << 2;
        // RW
        const FRAME_ROLL_ENABLE = 1 << 3;
        // RW
        const HOST_ERROR_ENABLE = 1 << 4;
        // RW
        const ASYNC_ADVANCE_ENABLE = 1 << 5;
    }
}
#[derive(Debug)]
pub enum EhciPortSpeed {
    High,
    Low,
    Full,
}
