//! We provide two RAM implementations: `ram_eager`, which eagerly allocates
//! the full ~4GiB in a single Vec, and `ram_lazy`, which lazily allocates on
//! a page-by-page basis in a HashMap of Vecs. See `README.md` for more info.
//!
//! Both implementations provide a `RAM::new()` function, and implement
//!`Index<usize>` and `IndexMut<usize>`. This is their public interface.

#[cfg(not(feature = "lazy-ram"))]
mod ram_eager;
#[cfg(not(feature = "lazy-ram"))]
pub use ram_eager::RAM;

#[cfg(feature = "lazy-ram")]
mod ram_lazy;
#[cfg(feature = "lazy-ram")]
pub use ram_lazy::RAM;
