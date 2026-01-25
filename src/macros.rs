// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

#[macro_export]
macro_rules! declare_com_interface {
    (
        $(#[$interface_attr:meta])*
        pub trait $trait_name:ident: IUnknown {
            const IID: GUID = $guid:expr;
            $($methods:tt)*
        }
    ) => {
        $crate::__kcom_define_interface! {
            @entry
            attrs [$(#[$interface_attr])*],
            trait_name $trait_name,
            parent_trait ($crate::IUnknown),
            parent_vtable (<
                $crate::IUnknownInterface as $crate::traits::ComInterfaceInfo
            >::Vtable),
            iid ($guid),
            methods { $($methods)* }
        }
    };
    (
        $(#[$interface_attr:meta])*
        pub trait $trait_name:ident: $parent_trait:ident {
            const IID: GUID = $guid:expr;
            $($methods:tt)*
        }
    ) => {
        $crate::__kcom_define_interface! {
            @entry
            attrs [$(#[$interface_attr])*],
            trait_name $trait_name,
            parent_trait ($parent_trait),
            parent_vtable (<
                [<$parent_trait Interface>] as $crate::traits::ComInterfaceInfo
            >::Vtable),
            iid ($guid),
            methods { $($methods)* }
        }
    };
}

#[macro_export]
/// Implements `ComImpl::query_interface` for a single primary interface.
///
/// Additional interfaces must provide explicit pointers (tear-offs or aggregated objects).
/// Returning `this` is only valid when the caller will interpret the vtable at offset 0
/// as the requested interface.
macro_rules! impl_query_interface {
    (
        $ty:ty,
        $this:ident,
        $riid:ident,
        [$primary:ident $(=> $primary_ptr:expr)? $(, $iface:ident => $ptr:expr)* $(,)?],
        fallback = $fallback:ty $(,)?
    ) => {
        #[inline]
        fn query_interface(
            &self,
            $this: *mut core::ffi::c_void,
            $riid: &$crate::GUID,
        ) -> Option<*mut core::ffi::c_void> {
            $crate::paste::paste! {
                if *$riid == <[<$primary Interface>] as $crate::traits::ComInterfaceInfo>::IID {
                    return $crate::impl_query_interface!(@return $this $(, $primary_ptr)?);
                }
                $(
                    if *$riid == <[<$iface Interface>] as $crate::traits::ComInterfaceInfo>::IID {
                        return $crate::impl_query_interface!(@return $this, $ptr);
                    }
                )*
            }
            <Self as $crate::traits::ComImpl<$fallback>>::query_interface(self, $this, $riid)
        }
    };
    (@return $this:ident, $ptr:expr) => {{
        let ptr = $ptr as *mut core::ffi::c_void;
        if ptr.is_null() {
            None
        } else {
            Some(ptr)
        }
    }};
    (@return $this:ident) => {{
        Some($this)
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __kcom_define_interface {
    (@entry
        attrs [$($attrs:tt)*],
        trait_name $trait_name:ident,
        parent_trait ($parent_trait:path),
        parent_vtable ($($parent_vtable:tt)+),
        iid ($guid:expr),
        methods { $($methods:tt)* }
    ) => {
        $crate::__kcom_define_interface!(
            @parse
            attrs [$($attrs)*],
            trait_name $trait_name,
            parent_trait ($parent_trait),
            parent_vtable ($($parent_vtable)+),
            iid ($guid),
            trait_methods [],
            vtable_fields [],
            shim_funcs [],
            ;
            $($methods)*
        );
    };

    (@parse
        attrs [$($attrs:tt)*],
        trait_name $trait_name:ident,
        parent_trait ($parent_trait:path),
        parent_vtable ($($parent_vtable:tt)+),
        iid ($guid:expr),
        trait_methods [$($trait_methods:tt)*],
        vtable_fields [$($vtable_fields:tt)*],
        shim_funcs [$($shim_funcs:tt)*],
        ;
    ) => {
        $($attrs)*
        pub trait $trait_name: $parent_trait + Sync {
            const IID: $crate::GUID = $guid;
            $($trait_methods)*
        }

        $crate::paste::paste! {
            #[repr(C)]
            #[derive(Clone, Copy)]
            #[allow(non_snake_case)]
            pub struct [<$trait_name Vtbl>] {
                pub parent: $($parent_vtable)+,
                $($vtable_fields)*
            }

            unsafe impl $crate::traits::InterfaceVtable for [<$trait_name Vtbl>] {}

            pub struct [<$trait_name Interface>];

            impl $crate::traits::ComInterfaceInfo for [<$trait_name Interface>] {
                type Vtable = [<$trait_name Vtbl>];
                const IID: $crate::GUID = $guid;
            }

            $($shim_funcs)*
        }
    };

    (@parse
        attrs [$($attrs:tt)*],
        trait_name $trait_name:ident,
        parent_trait ($parent_trait:path),
        parent_vtable ($($parent_vtable:tt)+),
        iid ($guid:expr),
        trait_methods [$($trait_methods:tt)*],
        vtable_fields [$($vtable_fields:tt)*],
        shim_funcs [$($shim_funcs:tt)*],
        ;
        $(#[$method_attr:meta])* async fn $method_name:ident(&self $(, $arg_name:ident : $arg_ty:ty)*) -> $ret_ty:ty; $($rest:tt)*
    ) => {
        $crate::__kcom_define_interface!(
            @parse
            attrs [$($attrs)*],
            trait_name $trait_name,
            parent_trait ($parent_trait),
            parent_vtable ($($parent_vtable)+),
            iid ($guid),
            trait_methods [
                $($trait_methods)*
                $(#[$method_attr])*
                #[cfg(feature = "async-com")]
                fn $method_name(&self $(, $arg_name : $arg_ty)*) -> ::core::pin::Pin<
                    $crate::alloc::boxed::Box<
                        dyn ::core::future::Future<Output = $ret_ty> + Send + '_
                    >
                >;
            ],
            vtable_fields [
                $($vtable_fields)*
                #[cfg(feature = "async-com")]
                pub $method_name: unsafe extern "system" fn(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $crate::__kcom_vtable_ret!($ret_ty),
            ],
            shim_funcs [
                $($shim_funcs)*
                #[cfg(not(feature = "async-com"))]
                compile_error!("async-com feature is required to use async methods in declare_com_interface!");
                #[cfg(feature = "async-com")]
                #[allow(non_snake_case)]
                unsafe extern "system" fn [<shim_ $trait_name _ $method_name>]<T: $trait_name>(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $crate::__kcom_vtable_ret!($ret_ty)
                where
                    T: $crate::ComImpl<[<$trait_name Vtbl>]>,
                {
                    let wrapper = unsafe {
                        $crate::wrapper::ComObject::<T, [<$trait_name Vtbl>]>::from_ptr(this)
                    };
                    let result = $crate::executor::block_on(wrapper.inner.$method_name($($arg_name),*));
                    $crate::__kcom_map_return!($ret_ty, result)
                }
            ],
            ;
            $($rest)*
        );
    };

    (@parse
        attrs [$($attrs:tt)*],
        trait_name $trait_name:ident,
        parent_trait ($parent_trait:path),
        parent_vtable ($($parent_vtable:tt)+),
        iid ($guid:expr),
        trait_methods [$($trait_methods:tt)*],
        vtable_fields [$($vtable_fields:tt)*],
        shim_funcs [$($shim_funcs:tt)*],
        ;
        $(#[$method_attr:meta])* fn $method_name:ident(&self $(, $arg_name:ident : $arg_ty:ty)*) -> Result<$ok:ty, $err:ty>; $($rest:tt)*
    ) => {
        $crate::__kcom_define_interface!(
            @parse
            attrs [$($attrs)*],
            trait_name $trait_name,
            parent_trait ($parent_trait),
            parent_vtable ($($parent_vtable)+),
            iid ($guid),
            trait_methods [
                $($trait_methods)*
                $(#[$method_attr])* fn $method_name(&self $(, $arg_name : $arg_ty)*) -> Result<$ok, $err>;
            ],
            vtable_fields [
                $($vtable_fields)*
                pub $method_name: unsafe extern "system" fn(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $crate::NTSTATUS,
            ],
            shim_funcs [
                $($shim_funcs)*
                #[allow(non_snake_case)]
                unsafe extern "system" fn [<shim_ $trait_name _ $method_name>]<T: $trait_name>(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $crate::NTSTATUS
                where
                    T: $crate::ComImpl<[<$trait_name Vtbl>]>,
                {
                    let wrapper = unsafe {
                        $crate::wrapper::ComObject::<T, [<$trait_name Vtbl>]>::from_ptr(this)
                    };
                    $crate::iunknown::IntoNtStatus::into_ntstatus(wrapper.inner.$method_name($($arg_name),*))
                }
            ],
            ;
            $($rest)*
        );
    };

    (@parse
        attrs [$($attrs:tt)*],
        trait_name $trait_name:ident,
        parent_trait ($parent_trait:path),
        parent_vtable ($($parent_vtable:tt)+),
        iid ($guid:expr),
        trait_methods [$($trait_methods:tt)*],
        vtable_fields [$($vtable_fields:tt)*],
        shim_funcs [$($shim_funcs:tt)*],
        ;
        $(#[$method_attr:meta])* fn $method_name:ident(&self $(, $arg_name:ident : $arg_ty:ty)*) -> ::core::result::Result<$ok:ty, $err:ty>; $($rest:tt)*
    ) => {
        $crate::__kcom_define_interface!(
            @parse
            attrs [$($attrs)*],
            trait_name $trait_name,
            parent_trait ($parent_trait),
            parent_vtable ($($parent_vtable)+),
            iid ($guid),
            trait_methods [
                $($trait_methods)*
                $(#[$method_attr])* fn $method_name(&self $(, $arg_name : $arg_ty)*) -> ::core::result::Result<$ok, $err>;
            ],
            vtable_fields [
                $($vtable_fields)*
                pub $method_name: unsafe extern "system" fn(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $crate::NTSTATUS,
            ],
            shim_funcs [
                $($shim_funcs)*
                #[allow(non_snake_case)]
                unsafe extern "system" fn [<shim_ $trait_name _ $method_name>]<T: $trait_name>(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $crate::NTSTATUS
                where
                    T: $crate::ComImpl<[<$trait_name Vtbl>]>,
                {
                    let wrapper = unsafe {
                        $crate::wrapper::ComObject::<T, [<$trait_name Vtbl>]>::from_ptr(this)
                    };
                    $crate::iunknown::IntoNtStatus::into_ntstatus(wrapper.inner.$method_name($($arg_name),*))
                }
            ],
            ;
            $($rest)*
        );
    };

    (@parse
        attrs [$($attrs:tt)*],
        trait_name $trait_name:ident,
        parent_trait ($parent_trait:path),
        parent_vtable ($($parent_vtable:tt)+),
        iid ($guid:expr),
        trait_methods [$($trait_methods:tt)*],
        vtable_fields [$($vtable_fields:tt)*],
        shim_funcs [$($shim_funcs:tt)*],
        ;
        $(#[$method_attr:meta])* fn $method_name:ident(&self $(, $arg_name:ident : $arg_ty:ty)*) -> ::std::result::Result<$ok:ty, $err:ty>; $($rest:tt)*
    ) => {
        $crate::__kcom_define_interface!(
            @parse
            attrs [$($attrs)*],
            trait_name $trait_name,
            parent_trait ($parent_trait),
            parent_vtable ($($parent_vtable)+),
            iid ($guid),
            trait_methods [
                $($trait_methods)*
                $(#[$method_attr])* fn $method_name(&self $(, $arg_name : $arg_ty)*) -> ::std::result::Result<$ok, $err>;
            ],
            vtable_fields [
                $($vtable_fields)*
                pub $method_name: unsafe extern "system" fn(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $crate::NTSTATUS,
            ],
            shim_funcs [
                $($shim_funcs)*
                #[allow(non_snake_case)]
                unsafe extern "system" fn [<shim_ $trait_name _ $method_name>]<T: $trait_name>(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $crate::NTSTATUS
                where
                    T: $crate::ComImpl<[<$trait_name Vtbl>]>,
                {
                    let wrapper = unsafe {
                        $crate::wrapper::ComObject::<T, [<$trait_name Vtbl>]>::from_ptr(this)
                    };
                    $crate::iunknown::IntoNtStatus::into_ntstatus(wrapper.inner.$method_name($($arg_name),*))
                }
            ],
            ;
            $($rest)*
        );
    };

    (@parse
        attrs [$($attrs:tt)*],
        trait_name $trait_name:ident,
        parent_trait ($parent_trait:path),
        parent_vtable ($($parent_vtable:tt)+),
        iid ($guid:expr),
        trait_methods [$($trait_methods:tt)*],
        vtable_fields [$($vtable_fields:tt)*],
        shim_funcs [$($shim_funcs:tt)*],
        ;
        $(#[$method_attr:meta])* fn $method_name:ident(&self $(, $arg_name:ident : $arg_ty:ty)*) -> $ret_ty:ty; $($rest:tt)*
    ) => {
        $crate::__kcom_define_interface!(
            @parse
            attrs [$($attrs)*],
            trait_name $trait_name,
            parent_trait ($parent_trait),
            parent_vtable ($($parent_vtable)+),
            iid ($guid),
            trait_methods [
                $($trait_methods)*
                $(#[$method_attr])* fn $method_name(&self $(, $arg_name : $arg_ty)*) -> $ret_ty;
            ],
            vtable_fields [
                $($vtable_fields)*
                pub $method_name: unsafe extern "system" fn(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $ret_ty,
            ],
            shim_funcs [
                $($shim_funcs)*
                #[allow(non_snake_case)]
                unsafe extern "system" fn [<shim_ $trait_name _ $method_name>]<T: $trait_name>(
                    this: *mut core::ffi::c_void
                    $(, $arg_name: $arg_ty)*
                ) -> $ret_ty
                where
                    T: $crate::ComImpl<[<$trait_name Vtbl>]>,
                {
                    let wrapper = unsafe {
                        $crate::wrapper::ComObject::<T, [<$trait_name Vtbl>]>::from_ptr(this)
                    };
                    $crate::__kcom_map_return!($ret_ty, wrapper.inner.$method_name($($arg_name),*))
                }
            ],
            ;
            $($rest)*
        );
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __kcom_vtable_ret {
    (Result<$ok:ty, $err:ty>) => {
        $crate::NTSTATUS
    };
    (::core::result::Result<$ok:ty, $err:ty>) => {
        $crate::NTSTATUS
    };
    (::std::result::Result<$ok:ty, $err:ty>) => {
        $crate::NTSTATUS
    };
    (core::result::Result<$ok:ty, $err:ty>) => {
        $crate::NTSTATUS
    };
    (std::result::Result<$ok:ty, $err:ty>) => {
        $crate::NTSTATUS
    };
    ($ret_ty:ty) => {
        $ret_ty
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __kcom_map_return {
    (Result<$ok:ty, $err:ty>, $expr:expr) => {
        $crate::iunknown::IntoNtStatus::into_ntstatus($expr)
    };
    (::core::result::Result<$ok:ty, $err:ty>, $expr:expr) => {
        $crate::iunknown::IntoNtStatus::into_ntstatus($expr)
    };
    (::std::result::Result<$ok:ty, $err:ty>, $expr:expr) => {
        $crate::iunknown::IntoNtStatus::into_ntstatus($expr)
    };
    (core::result::Result<$ok:ty, $err:ty>, $expr:expr) => {
        $crate::iunknown::IntoNtStatus::into_ntstatus($expr)
    };
    (std::result::Result<$ok:ty, $err:ty>, $expr:expr) => {
        $crate::iunknown::IntoNtStatus::into_ntstatus($expr)
    };
    (NTSTATUS, $expr:expr) => {
        $crate::iunknown::IntoNtStatus::into_ntstatus($expr)
    };
    ($crate::NTSTATUS, $expr:expr) => {
        $crate::iunknown::IntoNtStatus::into_ntstatus($expr)
    };
    ($ret_ty:ty, $expr:expr) => {
        $expr
    };
}
