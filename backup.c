    qemu_log(
        "\n=== EHCI DEBUG: FETCHQH ===\n"
        " entry=0x%08x async=%d qhaddr=0x%08x\n"
        " next=0x%08x curqtd=0x%08x next_qtd=0x%08x altnext_qtd=0x%08x\n"
        " token=0x%08x epchar=0x%08x epcap=0x%08x\n"
        " overlay: bufptr0=0x%08x bufptr1=0x%08x bufptr2=0x%08x\n"
        " devaddr=%d ep=%d eps=%d mps=%d HBIT=%d DTC=%d Ibit=%d\n",
        entry, async, NLPTR_GET(q->qhaddr),
        qh.next, qh.current_qtd, qh.next_qtd, qh.altnext_qtd,
        qh.token, qh.epchar, qh.epcap, 
        qh.bufptr[0], qh.bufptr[1], qh.bufptr[2],
        get_field(qh.epchar, QH_EPCHAR_DEVADDR),
        get_field(qh.epchar, QH_EPCHAR_EP),
        get_field(qh.epchar, QH_EPCHAR_EPS),
        get_field(qh.epchar, QH_EPCHAR_MPLEN),
        !!(qh.epchar & QH_EPCHAR_H),
        !!(qh.epchar & QH_EPCHAR_DTC),
        !!(qh.epchar & QH_EPCHAR_I)
    );

