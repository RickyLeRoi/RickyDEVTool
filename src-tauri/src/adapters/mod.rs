// Adapter OS-specifici: tutto il codice #[cfg(target_os)] vive qui dentro.
pub mod accessibility;
pub mod clipboard;
pub mod disks;
pub mod docker;
pub mod kill;
pub mod ports;
pub mod procs;
pub mod tools;
