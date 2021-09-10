//! We provide two RAM implementations: `ram_eager`, which eagerly allocates
//! the full ~4GiB in a single Vec, and `ram_lazy`, which lazily allocates on
//! a page-by-page basis in a HashMap of Vecs.
//!
//! Note that `ram_eager` behaves differently on different platforms, while
//! `ram_lazy` always behaves the same.
//! On Linux, optimistic memory allocation with demand paging is on by default;
//! this means that `ram_eager` will only consume physical memory when actually
//! written to, yielding a simple and performant implementation. Other platforms
//! do not support this, and will eagerly allocate physical memory, making
//! the allocation far more likely to fail.
//!
//! The default is `ram_eager`, but it you have a non-linux system without
//! 4GiB of memory readily available, you may need to enable the `lazy-ram`
//! feature to use the lazy implementation.
//!
//! Both implementations provide a `RAM::new()` function, and implement
//! `Index<usize>` and `IndexMut<usize>`. This is their public interface.

#[cfg(not(feature = "lazy-ram"))]
mod ram_eager;
#[cfg(not(feature = "lazy-ram"))]
pub use ram_eager::RAM;

#[cfg(feature = "lazy-ram")]
mod ram_lazy;
#[cfg(feature = "lazy-ram")]
pub use ram_lazy::RAM;
