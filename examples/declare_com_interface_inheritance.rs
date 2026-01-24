use kcom::{
    declare_com_interface, impl_com_object, impl_query_interface, ComImpl, ComObject, GUID,
    IUnknownVtbl, NTSTATUS, STATUS_SUCCESS,
};

declare_com_interface! {
    /// Base interface.
    pub trait IMiniport: IUnknown {
        const IID: GUID = GUID {
            data1: 0x1111_2222,
            data2: 0x3333,
            data3: 0x4444,
            data4: [0x55, 0x66, 0x77, 0x88, 0x99, 0xAA, 0xBB, 0xCC],
        };

        fn init(&self) -> NTSTATUS;
    }
}

declare_com_interface! {
    /// Derived interface (extends IMiniport).
    pub trait IMiniportWaveRT: IMiniport {
        const IID: GUID = GUID {
            data1: 0xAAAA_BBBB,
            data2: 0xCCCC,
            data3: 0xDDDD,
            data4: [0xEE, 0xFF, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60],
        };

        fn new_stream(&self, id: u32) -> NTSTATUS;
    }
}

struct Miniport;

impl IMiniport for Miniport {
    fn init(&self) -> NTSTATUS {
        STATUS_SUCCESS
    }
}

impl IMiniportWaveRT for Miniport {
    fn new_stream(&self, _id: u32) -> NTSTATUS {
        STATUS_SUCCESS
    }
}

impl ComImpl<IMiniportVtbl> for Miniport {
    const VTABLE: &'static IMiniportVtbl = &IMiniportVtbl {
        parent: *<Miniport as ComImpl<IUnknownVtbl>>::VTABLE,
        init: shim_IMiniport_init::<Miniport>,
    };

    impl_query_interface! {
        Self,
        this,
        riid,
        [IMiniport],
        fallback = IUnknownVtbl
    }
}

impl ComImpl<IMiniportWaveRTVtbl> for Miniport {
    const VTABLE: &'static IMiniportWaveRTVtbl = &IMiniportWaveRTVtbl {
        parent: *<Miniport as ComImpl<IMiniportVtbl>>::VTABLE,
        new_stream: shim_IMiniportWaveRT_new_stream::<Miniport>,
    };

    impl_query_interface! {
        Self,
        this,
        riid,
        [IMiniportWaveRT],
        fallback = IMiniportVtbl
    }
}

impl_com_object!(Miniport, IMiniportWaveRTVtbl);

fn main() {
    let raw = Miniport::new_com(Miniport);

    unsafe {
        let vtbl = *(raw as *mut *const IMiniportWaveRTVtbl);
        let init_status = ((*vtbl).parent.init)(raw);
        let stream_status = ((*vtbl).new_stream)(raw, 7);
        assert_eq!(init_status, STATUS_SUCCESS);
        assert_eq!(stream_status, STATUS_SUCCESS);

        ComObject::<Miniport, IMiniportWaveRTVtbl>::shim_release(raw);
    }
}
