//! Rust access to the official Wooting RGB SDK, built from the pinned submodule.
//!
//! A single [`Sdk`] owns the process-wide C state. Its methods require exclusive
//! access. Share it between threads through a `Mutex<Sdk>`. Device handles borrow
//! it, keeping device selection and a sequence of commands together.
//!
//! ```no_run
//! use wooting_rgb_rust::{Sdk, Frame, Rgb};
//! let mut sdk = Sdk::new()?;
//! let mut keyboard = sdk.device(0)?;
//! let info = keyboard.info()?;
//! let mut frame = Frame::new();
//! frame.set(1, 2, Rgb::new(255, 80, 0))?;
//! keyboard.set_frame(&frame)?;
//! keyboard.flush()?;
//! # Ok::<(), wooting_rgb_rust::Error>(())
//! ```

use std::{
    cell::Cell,
    ffi::CStr,
    fmt,
    marker::PhantomData,
    sync::atomic::{AtomicBool, Ordering},
};

/// Low-level bindings generated from the pinned upstream headers.
///
/// # Safety
/// Callers must serialize ALL native calls, including calls through `Sdk`.
/// Initialize discovery before accessing metadata, obey C buffer sizes and
/// lifetimes, and validate indexes. Never mix raw calls with a live safe `Sdk`.
/// Callbacks must not unwind or reenter the SDK. Prefer the safe API.
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    clippy::all
)]
pub mod sys {
    include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
}

#[cfg(not(test))]
use sys as native;
#[cfg(test)]
#[path = "test_backend.rs"]
mod native;

pub const ROWS: usize = sys::WOOTING_RGB_ROWS as usize;
pub const COLUMNS: usize = sys::WOOTING_RGB_COLS as usize;
pub const FRAME_BYTES: usize = ROWS * COLUMNS * 3;
static IN_USE: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    AlreadyInUse,
    DeviceUnavailable(u8),
    InvalidCoordinate { row: usize, column: usize },
    InvalidFrameLength { expected: usize, actual: usize },
    InvalidMetadata,
    OperationFailed(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AlreadyInUse => write!(f, "a Wooting SDK instance is already active"),
            Self::DeviceUnavailable(i) => write!(f, "Wooting device {i} is unavailable"),
            Self::InvalidCoordinate { row, column } => {
                write!(f, "RGB coordinate ({row}, {column}) is out of bounds")
            }
            Self::InvalidFrameLength { expected, actual } => {
                write!(f, "expected {expected} RGB bytes, received {actual}")
            }
            Self::InvalidMetadata => write!(f, "SDK returned invalid device metadata"),
            Self::OperationFailed(op) => write!(f, "Wooting SDK failed to {op}"),
        }
    }
}
impl std::error::Error for Error {}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}
impl Rgb {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

/// A row-major 6 × 21 RGB frame. Smaller devices still use the 21-column stride.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    bytes: [u8; FRAME_BYTES],
}
impl Default for Frame {
    fn default() -> Self {
        Self::new()
    }
}
impl Frame {
    pub const fn new() -> Self {
        Self {
            bytes: [0; FRAME_BYTES],
        }
    }
    pub fn fill(&mut self, color: Rgb) {
        for pixel in self.bytes.as_chunks_mut::<3>().0 {
            pixel.copy_from_slice(&[color.red, color.green, color.blue]);
        }
    }
    pub fn set(&mut self, row: usize, column: usize, color: Rgb) -> Result<(), Error> {
        validate_coordinate(row, column, ROWS, COLUMNS)?;
        let offset = (row * COLUMNS + column) * 3;
        self.bytes[offset..offset + 3].copy_from_slice(&[color.red, color.green, color.blue]);
        Ok(())
    }
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        let bytes = bytes.try_into().map_err(|_| Error::InvalidFrameLength {
            expected: FRAME_BYTES,
            actual: bytes.len(),
        })?;
        Ok(Self { bytes })
    }
    pub fn as_bytes(&self) -> &[u8; FRAME_BYTES] {
        &self.bytes
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    Tkl,
    FullSize,
    SixtyPercent,
    Keypad,
    EightyPercent,
    Unknown(i64),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Unknown,
    Ansi,
    Iso,
    Jis,
    AnsiSplit,
    IsoSplit,
    Other(i32),
}

/// Owned snapshot. Matrix dimensions include unused positions and spacer rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub index: u8,
    pub model: String,
    pub rows: u8,
    pub columns: u8,
    pub led_index_max: u8,
    pub device_type: DeviceType,
    pub layout: Layout,
    pub v2_interface: bool,
    pub uses_small_packets: bool,
    pub uses_multi_report: bool,
}

/// Exclusive owner of the native SDK. Dropping restores RGB and closes devices.
///
/// `Sdk` can be moved between threads. All access needs `&mut self`, so an
/// application can place it in a mutex to coordinate its UI and rendering thread.
pub struct Sdk {
    _not_sync: PhantomData<Cell<()>>,
}
impl Sdk {
    /// Initialize discovery. No attached keyboard is a valid empty device list.
    pub fn new() -> Result<Self, Error> {
        IN_USE
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| Error::AlreadyInUse)?;
        // SAFETY: We exclusively own the C globals. Discovery initializes metadata
        // even when no keyboard is attached. Auto-update has process-wide state.
        unsafe {
            native::wooting_rgb_kbd_connected();
            native::wooting_rgb_array_auto_update(false);
        }
        Ok(Self {
            _not_sync: PhantomData,
        })
    }
    pub fn devices(&mut self) -> Result<Vec<DeviceInfo>, Error> {
        // SAFETY: Exclusive access to initialized SDK state.
        let count = unsafe { native::wooting_usb_device_count() };
        (0..count).map(device_info).collect()
    }
    /// Restore and close existing devices, then discover again. Indexes may change.
    pub fn refresh(&mut self) -> Result<Vec<DeviceInfo>, Error> {
        // SAFETY: No device handle can coexist with this mutable borrow.
        unsafe {
            native::wooting_rgb_close();
            native::wooting_rgb_kbd_connected();
            native::wooting_rgb_array_auto_update(false);
        }
        self.devices()
    }
    pub fn device(&mut self, index: u8) -> Result<Device<'_>, Error> {
        // SAFETY: The native function validates the index before selecting buffers.
        if !unsafe { native::wooting_usb_select_device(index) } {
            return Err(Error::DeviceUnavailable(index));
        }
        device_info(index)?;
        Ok(Device { _sdk: self, index })
    }
    /// Explicitly restore every keyboard and close the SDK, reporting reset errors.
    pub fn close(self) -> Result<(), Error> {
        self.reset_all()
    }
    fn reset_all(&self) -> Result<(), Error> {
        let mut success = true;
        // SAFETY: This instance uniquely owns native state, including at drop.
        unsafe {
            let count = native::wooting_usb_device_count();
            for index in 0..count {
                if native::wooting_usb_select_device(index) {
                    success &= native::wooting_rgb_reset_rgb();
                } else {
                    success = false;
                }
            }
            native::wooting_usb_disconnect(false);
        }
        check(success, "restore keyboard colors")
    }
}
impl Drop for Sdk {
    fn drop(&mut self) {
        let _ = self.reset_all();
        IN_USE.store(false, Ordering::Release);
    }
}

pub struct Device<'a> {
    _sdk: &'a mut Sdk,
    index: u8,
}
impl Device<'_> {
    pub fn info(&mut self) -> Result<DeviceInfo, Error> {
        device_info(self.index)
    }
    fn ready(&mut self) -> Result<DeviceInfo, Error> {
        // The borrow retains sole access. Check metadata before C functions that
        // otherwise rediscover devices and might send to a different keyboard.
        device_info(self.index)
    }
    pub fn set_key(&mut self, row: usize, column: usize, color: Rgb) -> Result<(), Error> {
        let info = self.ready()?;
        validate_coordinate(row, column, info.rows.into(), info.columns.into())?;
        // SAFETY: Both device and fixed native matrix bounds have been checked.
        check(
            unsafe {
                native::wooting_rgb_array_set_single(
                    row as u8,
                    column as u8,
                    color.red,
                    color.green,
                    color.blue,
                )
            },
            "buffer key color",
        )
    }
    pub fn set_key_direct(&mut self, row: usize, column: usize, color: Rgb) -> Result<(), Error> {
        let info = self.ready()?;
        validate_coordinate(row, column, info.rows.into(), info.columns.into())?;
        // SAFETY: Connected selected device and validated matrix coordinates.
        check(
            unsafe {
                native::wooting_rgb_direct_set_key(
                    row as u8,
                    column as u8,
                    color.red,
                    color.green,
                    color.blue,
                )
            },
            "set key color",
        )
    }
    pub fn reset_key(&mut self, row: usize, column: usize) -> Result<(), Error> {
        let info = self.ready()?;
        validate_coordinate(row, column, info.rows.into(), info.columns.into())?;
        // SAFETY: Connected selected device and validated matrix coordinates.
        check(
            unsafe { native::wooting_rgb_direct_reset_key(row as u8, column as u8) },
            "reset key color",
        )
    }
    pub fn set_frame(&mut self, frame: &Frame) -> Result<(), Error> {
        self.ready()?;
        // SAFETY: Frame always owns all 378 bytes consumed by the C implementation.
        check(
            unsafe { native::wooting_rgb_array_set_full(frame.bytes.as_ptr()) },
            "buffer RGB frame",
        )
    }
    pub fn flush(&mut self) -> Result<(), Error> {
        self.ready()?;
        // SAFETY: A selected connected device owns the initialized RGB buffer.
        check(
            unsafe { native::wooting_rgb_array_update_keyboard() },
            "send RGB frame",
        )
    }
    pub fn reset(&mut self) -> Result<(), Error> {
        self.ready()?;
        // SAFETY: Exclusive access to selected connected device.
        check(
            unsafe { native::wooting_rgb_reset_rgb() },
            "restore keyboard colors",
        )
    }
    /// Enable immediate transmission after buffered changes. Applies to all devices.
    pub fn set_auto_update(&mut self, enabled: bool) {
        // SAFETY: Exclusive access to native SDK state.
        unsafe {
            native::wooting_rgb_array_auto_update(enabled);
        }
    }
    /// Send an SDK feature command and return the actual response bytes.
    ///
    /// Command semantics and response layout are firmware-specific. The wrapper
    /// sizes the native buffer for the selected protocol and rejects empty replies.
    pub fn feature_response(&mut self, command: u8, parameters: [u8; 4]) -> Result<Vec<u8>, Error> {
        self.ready()?;
        // SAFETY: Metadata was initialized, and the allocation exactly matches
        // the size of the upstream temporary buffer copied by this function.
        unsafe {
            let size = native::wooting_usb_get_response_size();
            let mut response = vec![0; size];
            let count = native::wooting_usb_send_feature_with_response(
                response.as_mut_ptr(),
                size,
                command,
                parameters[0],
                parameters[1],
                parameters[2],
                parameters[3],
            );
            if count <= 0 || count as usize > size {
                return Err(Error::OperationFailed("read feature response"));
            }
            response.truncate(count as usize);
            Ok(response)
        }
    }
}

fn device_info(index: u8) -> Result<DeviceInfo, Error> {
    // SAFETY: Private helper called only while an Sdk owns the native state.
    // No references to native memory escape. The model is copied before return.
    unsafe {
        let meta = native::wooting_usb_get_device_meta(index)
            .as_ref()
            .filter(|meta| meta.connected)
            .ok_or(Error::DeviceUnavailable(index))?;
        if meta.model.is_null()
            || meta.max_rows as usize > ROWS
            || meta.max_columns as usize > COLUMNS
        {
            return Err(Error::InvalidMetadata);
        }
        Ok(DeviceInfo {
            index,
            model: CStr::from_ptr(meta.model).to_string_lossy().into_owned(),
            rows: meta.max_rows,
            columns: meta.max_columns,
            led_index_max: meta.led_index_max,
            device_type: match i64::from(meta.device_type) {
                1 => DeviceType::Tkl,
                2 => DeviceType::FullSize,
                3 => DeviceType::SixtyPercent,
                4 => DeviceType::Keypad,
                5 => DeviceType::EightyPercent,
                n => DeviceType::Unknown(n),
            },
            layout: match meta.layout {
                -1 => Layout::Unknown,
                0 => Layout::Ansi,
                1 => Layout::Iso,
                2 => Layout::Jis,
                3 => Layout::AnsiSplit,
                4 => Layout::IsoSplit,
                n => Layout::Other(n),
            },
            v2_interface: meta.v2_interface,
            uses_small_packets: meta.uses_small_packets,
            uses_multi_report: meta.uses_multi_report,
        })
    }
}
fn check(success: bool, operation: &'static str) -> Result<(), Error> {
    if success {
        Ok(())
    } else {
        Err(Error::OperationFailed(operation))
    }
}
fn validate_coordinate(
    row: usize,
    column: usize,
    rows: usize,
    columns: usize,
) -> Result<(), Error> {
    if row >= rows || column >= columns || row >= ROWS || column >= COLUMNS {
        Err(Error::InvalidCoordinate { row, column })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frames_keep_native_stride_and_reject_bad_sizes() {
        let mut frame = Frame::new();
        frame.set(1, 0, Rgb::new(1, 2, 3)).unwrap();
        assert_eq!(&frame.as_bytes()[63..66], &[1, 2, 3]);
        frame.set(5, 20, Rgb::new(4, 5, 6)).unwrap();
        assert_eq!(&frame.as_bytes()[375..], &[4, 5, 6]);
        assert!(frame.set(6, 0, Rgb::default()).is_err());
        assert!(frame.set(0, 21, Rgb::default()).is_err());
        assert!(frame.set(usize::MAX, 0, Rgb::default()).is_err());
        assert!(Frame::from_bytes(&[0; 377]).is_err());
        assert!(Frame::from_bytes(&[0; 379]).is_err());
        assert_eq!(Frame::from_bytes(frame.as_bytes()).unwrap(), frame);
    }
    #[test]
    fn coordinates_respect_device_and_native_bounds() {
        assert!(validate_coordinate(5, 13, 6, 14).is_ok());
        assert!(validate_coordinate(5, 14, 6, 14).is_err());
        assert!(validate_coordinate(5, 0, 5, 7).is_err());
        assert!(validate_coordinate(6, 0, 255, 255).is_err());
        assert!(validate_coordinate(0, 0, 0, 0).is_err());
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn ownership_device_selection_disconnect_and_response_sizes() {
        let mut sdk = Sdk::new().unwrap();
        assert!(matches!(Sdk::new(), Err(Error::AlreadyInUse)));
        std::thread::spawn(|| assert!(matches!(Sdk::new(), Err(Error::AlreadyInUse))))
            .join()
            .unwrap();
        assert_eq!(sdk.devices().unwrap().len(), 2);
        assert!(matches!(sdk.device(2), Err(Error::DeviceUnavailable(2))));
        {
            let mut device = sdk.device(1).unwrap();
            assert_eq!(device.info().unwrap().index, 1);
            assert!(device.set_key(6, 0, Rgb::default()).is_err());
            assert!(device.set_key_direct(0, 14, Rgb::default()).is_err());
            assert!(device.reset_key(0, usize::MAX).is_err());
            device.set_key(5, 13, Rgb::new(1, 2, 3)).unwrap();
            let mut frame = Frame::new();
            frame.fill(Rgb::new(10, 20, 30));
            device.set_frame(&frame).unwrap();
            device.flush().unwrap();
            assert_eq!(native::last_frame(), frame.as_bytes());
            for (size, received) in [(128, 128), (256, 256), (2046, 33)] {
                native::response(size, received);
                assert_eq!(
                    device.feature_response(1, [0; 4]).unwrap().len(),
                    received as usize
                );
            }
            for received in [-1, 0, 2047] {
                native::response(2046, received);
                assert!(device.feature_response(1, [0; 4]).is_err());
            }
            native::disconnect();
            assert!(matches!(device.flush(), Err(Error::DeviceUnavailable(1))));
        }
        assert_eq!(sdk.refresh().unwrap().len(), 2);
        sdk.close().unwrap();
        assert_eq!(native::reset_indexes(), vec![0, 1]);
        let sdk = Sdk::new().unwrap();
        drop(sdk);
        assert!(Sdk::new().is_ok());
    }
}
