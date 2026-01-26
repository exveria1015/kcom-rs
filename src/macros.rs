// Copyright (c) 2026 Exveria
// SPDX-License-Identifier: MIT OR Apache-2.0

#[macro_export]
/// Declares a COM interface trait and generates its vtable definition.
///
/// When using `ComRc`, define a raw COM pointer struct (e.g., `IFooRaw` with an
/// `lpVtbl` field) and add `unsafe impl ComInterface for IFooRaw` to satisfy the
/// layout contract.
macro_rules! declare_com_interface {
    (
        $(#[$interface_attr:meta])*
        pub trait $trait_name:ident: IUnknown {
            const IID: $guid_ty:ty = $guid:expr;
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
            const IID: $guid_ty:ty = $guid:expr;
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
    (@return $this:ident, this) => {{
        Some($this)
    }};
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

#[macro_export]
/// Implements `ComImpl` and builds the VTABLE for a declared COM interface.
///
/// Defaults to using the primary interface for `QueryInterface` and the parent
/// vtable type as the fallback.
macro_rules! impl_com_interface {
    (
        impl $ty:ty: $trait_name:ident {
            parent = $parent_vtbl:ty,
            methods = [$($method:ident),* $(,)?],
            this = $this:ident,
            qi = [$($qi:tt)+],
            $(fallback = $fallback:ty,)?
        }
    ) => {
        $crate::impl_com_interface!(
            @impl
            $ty,
            $trait_name,
            $parent_vtbl,
            [$($method),*],
            [$($qi)+],
            $crate::impl_com_interface!(@fallback $parent_vtbl $(, $fallback)?),
            $this
        );
    };
    (
        impl $ty:ty: $trait_name:ident {
            parent = $parent_vtbl:ty,
            methods = [$($method:ident),* $(,)?],
            qi = [$($qi:tt)+],
            $(fallback = $fallback:ty,)?
        }
    ) => {
        $crate::impl_com_interface!(
            @impl
            $ty,
            $trait_name,
            $parent_vtbl,
            [$($method),*],
            [$($qi)+],
            $crate::impl_com_interface!(@fallback $parent_vtbl $(, $fallback)?),
            this
        );
    };
    (
        impl $ty:ty: $trait_name:ident {
            parent = $parent_vtbl:ty,
            methods = [$($method:ident),* $(,)?],
            $(fallback = $fallback:ty,)?
        }
    ) => {
        $crate::impl_com_interface!(
            @impl
            $ty,
            $trait_name,
            $parent_vtbl,
            [$($method),*],
            [$trait_name],
            $crate::impl_com_interface!(@fallback $parent_vtbl $(, $fallback)?),
            this
        );
    };
    (@fallback $parent_vtbl:ty) => { $parent_vtbl };
    (@fallback $parent_vtbl:ty, $fallback:ty) => { $fallback };
    (@parent IUnknownVtbl, $ty:ty, $trait_name:ident) => {{
        $crate::paste::paste! {
            $crate::IUnknownVtbl {
                QueryInterface: $crate::wrapper::ComObject::<$ty, [<$trait_name Vtbl>]>::shim_query_interface,
                AddRef: $crate::wrapper::ComObject::<$ty, [<$trait_name Vtbl>]>::shim_add_ref,
                Release: $crate::wrapper::ComObject::<$ty, [<$trait_name Vtbl>]>::shim_release,
            }
        }
    }};
    (@parent $parent_vtbl:ty, $ty:ty, $trait_name:ident) => {
        *<$ty as $crate::ComImpl<$parent_vtbl>>::VTABLE
    };
    (@impl
        $ty:ty,
        $trait_name:ident,
        $parent_vtbl:ty,
        [$($method:ident),*],
        [$($qi:tt)+],
        $fallback:ty,
        $this:ident
    ) => {
        $crate::paste::paste! {
            impl $crate::ComImpl<[<$trait_name Vtbl>]> for $ty {
                const VTABLE: &'static [<$trait_name Vtbl>] = &[<$trait_name Vtbl>] {
                    parent: $crate::impl_com_interface!(@parent $parent_vtbl, $ty, $trait_name),
                    $($method: [<shim_ $trait_name _ $method>]::<$ty>,)*
                };

                $crate::impl_query_interface! {
                    Self,
                    $this,
                    riid,
                    [$($qi)+],
                    fallback = $fallback
                }
            }
        }
    };
}

#[macro_export]
/// Returns early with `Err(status)` when `cond` is false.
macro_rules! ensure {
    ($cond:expr, $status:expr $(,)?) => {
        if !$cond {
            return Err($status);
        }
    };
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
            trait_docs [],
            trait_safety [],
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
        trait_docs [$($trait_docs:tt)*],
        trait_safety [$($trait_safety:tt)*],
        trait_methods [$($trait_methods:tt)*],
        vtable_fields [$($vtable_fields:tt)*],
        shim_funcs [$($shim_funcs:tt)*],
        ;
    ) => {
        $($attrs)*
        $($trait_docs)*
        pub $($trait_safety)* trait $trait_name: $parent_trait + Sync {
            #[allow(dead_code)]
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

            #[repr(C)]
            #[derive(Clone, Copy)]
            #[allow(non_snake_case)]
            pub struct [<$trait_name Raw>] {
                pub lpVtbl: *mut [<$trait_name Vtbl>],
            }

            unsafe impl $crate::ComInterface for [<$trait_name Raw>] {}

            impl $crate::traits::ComInterfaceInfo for [<$trait_name Raw>] {
                type Vtable = [<$trait_name Vtbl>];
                const IID: $crate::GUID = $guid;
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
        trait_docs [$($trait_docs:tt)*],
        trait_safety [$($trait_safety:tt)*],
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
            trait_docs [
                #[doc = "Async interfaces are unsafe to implement because the generated COM shims use a blocking executor."]
                #[doc = "Implementors must uphold block_on safety requirements (IRQL <= APC_LEVEL in kernel mode,"]
                #[doc = "avoid deadlocks, and ensure sufficient stack space)."]
            ],
            trait_safety [unsafe],
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
                    // SAFETY: caller of the shim must uphold block_on's safety requirements.
                    let result = unsafe {
                        $crate::executor::block_on(wrapper.inner.$method_name($($arg_name),*))
                    };
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
        trait_docs [$($trait_docs:tt)*],
        trait_safety [$($trait_safety:tt)*],
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
            trait_docs [$($trait_docs)*],
            trait_safety [$($trait_safety)*],
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
        trait_docs [$($trait_docs:tt)*],
        trait_safety [$($trait_safety:tt)*],
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
            trait_docs [$($trait_docs)*],
            trait_safety [$($trait_safety)*],
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
        trait_docs [$($trait_docs:tt)*],
        trait_safety [$($trait_safety:tt)*],
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
            trait_docs [$($trait_docs)*],
            trait_safety [$($trait_safety)*],
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
        trait_docs [$($trait_docs:tt)*],
        trait_safety [$($trait_safety:tt)*],
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
            trait_docs [$($trait_docs)*],
            trait_safety [$($trait_safety)*],
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
