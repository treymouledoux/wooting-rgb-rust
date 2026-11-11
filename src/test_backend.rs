//! Hardware-free regression fixture for the safe API. Production uses `sys`.
use crate::{sys::WOOTING_USB_META, FRAME_BYTES};
use std::cell::RefCell;

struct State {
    devices: Vec<WOOTING_USB_META>,
    selected: usize,
    response_size: usize,
    received: i32,
    frame: Vec<u8>,
    resets: Vec<u8>,
}
thread_local! {
    static STATE: RefCell<State> = const { RefCell::new(State {
        devices: Vec::new(), selected: 0, response_size: 256, received: 256,
        frame: Vec::new(), resets: Vec::new(),
    }) };
}

pub unsafe fn wooting_rgb_kbd_connected() -> bool {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.devices = (0..2)
            .map(|_| WOOTING_USB_META {
                connected: true,
                model: c"Test keyboard".as_ptr(),
                max_rows: 6,
                max_columns: 14,
                led_index_max: 116,
                device_type: 3,
                v2_interface: true,
                layout: 0,
                uses_small_packets: false,
                uses_multi_report: false,
            })
            .collect();
        true
    })
}
pub unsafe fn wooting_usb_device_count() -> u8 {
    STATE.with(|s| s.borrow().devices.len() as u8)
}
pub unsafe fn wooting_usb_get_device_meta(index: u8) -> *mut WOOTING_USB_META {
    STATE.with(|s| {
        s.borrow_mut()
            .devices
            .get_mut(index as usize)
            .map_or(std::ptr::null_mut(), |meta| meta as *mut _)
    })
}
pub unsafe fn wooting_usb_select_device(index: u8) -> bool {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        if index as usize >= s.devices.len() {
            return false;
        }
        s.selected = index as usize;
        true
    })
}
pub unsafe fn wooting_usb_disconnect(_: bool) {
    disconnect();
}
pub unsafe fn wooting_rgb_close() -> bool {
    disconnect();
    true
}
pub unsafe fn wooting_rgb_array_auto_update(_: bool) {}
pub unsafe fn wooting_rgb_reset_rgb() -> bool {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        let index = s.selected as u8;
        s.resets.push(index);
    });
    true
}
pub unsafe fn wooting_rgb_array_set_single(row: u8, column: u8, _: u8, _: u8, _: u8) -> bool {
    assert!(row < 6 && column < 14);
    true
}
pub unsafe fn wooting_rgb_direct_set_key(row: u8, column: u8, r: u8, g: u8, b: u8) -> bool {
    unsafe { wooting_rgb_array_set_single(row, column, r, g, b) }
}
pub unsafe fn wooting_rgb_direct_reset_key(row: u8, column: u8) -> bool {
    assert!(row < 6 && column < 14);
    true
}
pub unsafe fn wooting_rgb_array_set_full(bytes: *const u8) -> bool {
    let frame = unsafe { std::slice::from_raw_parts(bytes, FRAME_BYTES) }.to_vec();
    STATE.with(|s| s.borrow_mut().frame = frame);
    true
}
pub unsafe fn wooting_rgb_array_update_keyboard() -> bool {
    true
}
pub unsafe fn wooting_usb_get_response_size() -> usize {
    STATE.with(|s| s.borrow().response_size)
}
pub unsafe fn wooting_usb_send_feature_with_response(
    bytes: *mut u8,
    size: usize,
    _: u8,
    _: u8,
    _: u8,
    _: u8,
    _: u8,
) -> i32 {
    STATE.with(|s| {
        let s = s.borrow();
        assert_eq!(size, s.response_size);
        unsafe {
            std::ptr::write_bytes(bytes, 42, size);
        }
        s.received
    })
}
pub fn response(size: usize, received: i32) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.response_size = size;
        s.received = received;
    });
}
pub fn disconnect() {
    STATE.with(|s| s.borrow_mut().devices.clear());
}
pub fn last_frame() -> Vec<u8> {
    STATE.with(|s| s.borrow().frame.clone())
}
pub fn reset_indexes() -> Vec<u8> {
    STATE.with(|s| s.borrow().resets.clone())
}
