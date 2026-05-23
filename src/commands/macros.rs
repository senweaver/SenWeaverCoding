// SPDX-License-Identifier: MIT
// Copyright (c) 2025-2026 SenWeaverCoding
// Licensed under the MIT License.
#[macro_export]
macro_rules! make_handler {
    ($handler:expr) => {{

        static HANDLER_LAZY: ::std::sync::LazyLock<
            ::std::sync::Arc<
                dyn ::std::ops::Fn(
                        $crate::commands::registry::CommandContext,
                    ) -> ::std::pin::Pin<
                        ::std::boxed::Box<
                            dyn ::std::future::Future<
                                    Output = $crate::commands::registry::CommandResult,
                                > + ::std::marker::Send,
                        >,
                    > + ::std::marker::Send
                    + ::std::marker::Sync,
            >,
        > = ::std::sync::LazyLock::new(|| $crate::commands::registry::make_handler($handler));

        $crate::commands::registry::HandlerPtr::from_lazy_ptr(::std::ptr::addr_of!(HANDLER_LAZY))
    }};
}
