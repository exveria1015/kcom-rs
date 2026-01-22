use core::ffi::c_void;

use kcom::{
    declare_com_interface, impl_com_object, ComImpl, ComInterfaceInfo, ComObject, GUID, IUnknown,
    IUnknownVtbl, NTSTATUS, STATUS_SUCCESS,
};

declare_com_interface! {
    /// Minimal COM interface example.
    pub trait IFoo: IUnknown {
        const IID: GUID = GUID {
            data1: 0x1234_5678,
            data2: 0x1234,
            data3: 0x5678,
            data4: [0x90, 0xAB, 0xCD, 0xEF, 0x01, 0x23, 0x45, 0x67],
        };

        fn ping(&self, value: u32) -> NTSTATUS;
    }
}

struct Foo;

impl IFoo for Foo {
    fn ping(&self, _value: u32) -> NTSTATUS {
        STATUS_SUCCESS
    }
}

impl ComImpl<IFooVtbl> for Foo {
    const VTABLE: &'static IFooVtbl = &IFooVtbl {
        parent: *<Foo as ComImpl<IUnknownVtbl>>::VTABLE,
        ping: shim_IFoo_ping::<Foo>,
    };

    fn query_interface(&self, riid: &GUID) -> Option<*mut c_void> {
        if *riid == <IFooInterface as ComInterfaceInfo>::IID {
            Some(self as *const Foo as *mut c_void)
        } else {
            <Foo as ComImpl<IUnknownVtbl>>::query_interface(self, riid)
        }
    }
}

impl_com_object!(Foo, IFooVtbl);

fn main() {
    let raw = Foo::new_com(Foo);

    unsafe {
        let vtbl = *(raw as *mut *const IFooVtbl);
        let status = ((*vtbl).ping)(raw, 42);
        assert_eq!(status, STATUS_SUCCESS);

        ComObject::<Foo, IFooVtbl>::shim_release(raw);
    }
}
