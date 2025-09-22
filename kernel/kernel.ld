
/* Entry point symbol */
ENTRY(_start)

SECTIONS
{
    /* Define where we want the kernel to live */
    /* Limine maps physical 0x200000 at virtual 0xFFFFFFFF80000000 */
    KERNEL_VMA = 0xFFFFFFFF80000000;
    KERNEL_LMA = 0x200000;   /* physical load address (2 MiB) */

    . = KERNEL_VMA;

    .text : AT(KERNEL_LMA) ALIGN(4K)
    {
        KEEP(*(.text._start))
        *(.text .text.*)
    }

    .rodata : AT(ADDR(.rodata) - KERNEL_VMA + KERNEL_LMA) ALIGN(4K)
    {
        *(.rodata .rodata.*)
    }

    .data : AT(ADDR(.data) - KERNEL_VMA + KERNEL_LMA) ALIGN(4K)
    {
        *(.data .data.*)
    }

    .bss : AT(ADDR(.bss) - KERNEL_VMA + KERNEL_LMA) ALIGN(4K)
    {
        __bss_start = .;
        *(.bss .bss.* COMMON)
        __bss_end = .;
    }

    /DISCARD/ :
    {
        *(.comment)
        *(.note)
        *(.eh_frame)
    }
}
