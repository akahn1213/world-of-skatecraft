//! Adapted from the pinned skate-host platform_linux.rs; see ../NOTICE.
//! Changes: local XboxState, omit unused packet numbers/subtype and capability cache.
//! Linux evdev reports the same raw axes as XInput: xpad's Y inversion is undone.

use super::DeviceError;
use crate::XboxState;
use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const EV_SYN: u16 = 0x00;
const EV_KEY: u16 = 0x01;
const EV_ABS: u16 = 0x03;
const SYN_REPORT: u16 = 0;
const SYN_DROPPED: u16 = 3;
const ABS_X: u16 = 0x00;
const ABS_Y: u16 = 0x01;
const ABS_Z: u16 = 0x02;
const ABS_RX: u16 = 0x03;
const ABS_RY: u16 = 0x04;
const ABS_RZ: u16 = 0x05;
const ABS_GAS: u16 = 0x09;
const ABS_BRAKE: u16 = 0x0a;
const ABS_HAT0X: u16 = 0x10;
const ABS_HAT0Y: u16 = 0x11;
const ABS_COUNT: usize = 0x12;
const BTN_SOUTH: u16 = 0x130;
const BTN_TL2: u16 = 0x138;
const BTN_TR2: u16 = 0x139;
const KEY_MAX: usize = 0x2ff;

// evdev key codes to XInput wButtons, including Xbox face labels.
const BUTTONS: [(u16, u16); 14] = [
    (0x130, 0x1000),
    (0x131, 0x2000),
    (0x133, 0x4000),
    (0x134, 0x8000),
    (0x136, 0x0100),
    (0x137, 0x0200),
    (0x13a, 0x0020),
    (0x13b, 0x0010),
    (0x13d, 0x0040),
    (0x13e, 0x0080),
    (0x220, 0x0001),
    (0x221, 0x0002),
    (0x222, 0x0004),
    (0x223, 0x0008),
];
const DPAD_UP: u16 = 0x0001;
const DPAD_DOWN: u16 = 0x0002;
const DPAD_LEFT: u16 = 0x0004;
const DPAD_RIGHT: u16 = 0x0008;
const EVENT_SIZE: usize = 24;
const O_NONBLOCK: i32 = 0o4000;
const ENODEV: i32 = 19;

#[repr(C)]
#[derive(Clone, Copy, Default, Debug)]
struct AbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}
const _: () = assert!(size_of::<AbsInfo>() == 24);

unsafe extern "C" {
    fn ioctl(fd: std::ffi::c_int, request: std::ffi::c_ulong, ...) -> std::ffi::c_int;
}

// asm-generic _IOC(_IOC_READ, 'E', nr, size) on the supported 64-bit Linux targets.
const fn eviocg(nr: u32, size: usize) -> std::ffi::c_ulong {
    ((2u64 << 30) | ((size as u64) << 16) | ((b'E' as u64) << 8) | nr as u64) as std::ffi::c_ulong
}

fn ioctl_read(file: &File, request: std::ffi::c_ulong, out: &mut [u8]) -> bool {
    // SAFETY: eviocg encodes out.len(), so the kernel writes at most that many bytes.
    unsafe { ioctl(file.as_raw_fd(), request, out.as_mut_ptr()) >= 0 }
}

fn bit(bits: &[u8], index: usize) -> bool {
    bits.get(index / 8)
        .is_some_and(|byte| byte & (1 << (index % 8)) != 0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    right: [u16; 2],
    triggers: Option<[u16; 2]>,
}

impl Layout {
    fn from_axes(has: impl Fn(u16) -> bool) -> Self {
        let right = if has(ABS_RX) && has(ABS_RY) {
            [ABS_RX, ABS_RY]
        } else {
            [ABS_Z, ABS_RZ]
        };
        let triggers = if has(ABS_BRAKE) && has(ABS_GAS) {
            Some([ABS_BRAKE, ABS_GAS])
        } else if right != [ABS_Z, ABS_RZ] && has(ABS_Z) && has(ABS_RZ) {
            Some([ABS_Z, ABS_RZ])
        } else {
            None
        };
        Self { right, triggers }
    }
}

fn stick(value: i32, info: &AbsInfo, invert: bool) -> i16 {
    let raw = if info.minimum == i16::MIN as i32 && info.maximum == i16::MAX as i32 {
        value as i16
    } else if info.maximum > info.minimum {
        let span = i64::from(info.maximum) - i64::from(info.minimum);
        let offset = (i64::from(value) - i64::from(info.minimum)).clamp(0, span);
        (i64::from(i16::MIN) + (offset * 65535 + span / 2) / span) as i16
    } else {
        0
    };
    if invert { !raw } else { raw }
}

fn trigger(value: i32, info: &AbsInfo) -> u8 {
    if info.minimum == 0 && info.maximum == 255 {
        return value.clamp(0, 255) as u8;
    }
    if info.maximum <= info.minimum {
        return 0;
    }
    let span = i64::from(info.maximum) - i64::from(info.minimum);
    let offset = (i64::from(value) - i64::from(info.minimum)).clamp(0, span);
    ((offset * 255 + span / 2) / span) as u8
}

#[derive(Clone, Copy, Debug)]
struct Raw {
    buttons: u16,
    digital_triggers: [bool; 2],
    abs: [AbsInfo; ABS_COUNT],
}

impl Raw {
    fn key(&mut self, code: u16, pressed: bool) {
        if let Some(&(_, mask)) = BUTTONS.iter().find(|(key, _)| *key == code) {
            if pressed {
                self.buttons |= mask;
            } else {
                self.buttons &= !mask;
            }
        } else if code == BTN_TL2 {
            self.digital_triggers[0] = pressed;
        } else if code == BTN_TR2 {
            self.digital_triggers[1] = pressed;
        }
    }

    fn axis(&mut self, code: u16, value: i32) {
        if let Some(info) = self.abs.get_mut(usize::from(code)) {
            info.value = value;
        }
    }

    fn state(&self, layout: &Layout) -> XboxState {
        let abs = |code: u16| &self.abs[usize::from(code)];
        let mut buttons = self.buttons;
        let hat_x = abs(ABS_HAT0X).value;
        let hat_y = abs(ABS_HAT0Y).value;
        if hat_y < 0 {
            buttons |= DPAD_UP;
        }
        if hat_y > 0 {
            buttons |= DPAD_DOWN;
        }
        if hat_x < 0 {
            buttons |= DPAD_LEFT;
        }
        if hat_x > 0 {
            buttons |= DPAD_RIGHT;
        }
        let triggers = match layout.triggers {
            Some(codes) => codes.map(|code| trigger(abs(code).value, abs(code))),
            None => self
                .digital_triggers
                .map(|pressed| if pressed { 255 } else { 0 }),
        };
        let axis = |code: u16, invert: bool| stick(abs(code).value, abs(code), invert);
        XboxState {
            buttons,
            triggers,
            left: [axis(ABS_X, false), axis(ABS_Y, true)],
            right: [axis(layout.right[0], false), axis(layout.right[1], true)],
        }
    }
}

struct Decoder {
    layout: Layout,
    published: Raw,
    pending: Raw,
    dropped: bool,
}

enum Step {
    Continue,
    Resync,
}

impl Decoder {
    fn new(layout: Layout, raw: Raw) -> Self {
        Self {
            layout,
            published: raw,
            pending: raw,
            dropped: false,
        }
    }

    fn event(&mut self, kind: u16, code: u16, value: i32) -> Step {
        match (kind, code) {
            (EV_SYN, SYN_DROPPED) => {
                self.dropped = true;
                Step::Continue
            }
            (EV_SYN, SYN_REPORT) if self.dropped => {
                self.dropped = false;
                Step::Resync
            }
            (EV_SYN, SYN_REPORT) => {
                self.publish(self.pending);
                Step::Continue
            }
            _ if self.dropped => Step::Continue,
            (EV_KEY, _) => {
                self.pending.key(code, value != 0);
                Step::Continue
            }
            (EV_ABS, _) => {
                self.pending.axis(code, value);
                Step::Continue
            }
            _ => Step::Continue,
        }
    }

    fn publish(&mut self, raw: Raw) {
        self.published = raw;
        self.pending = raw;
    }

    fn packet(&self) -> XboxState {
        self.published.state(&self.layout)
    }
}

struct Pad {
    path: PathBuf,
    file: File,
    decoder: Decoder,
}

impl Pad {
    fn open(path: &Path) -> Option<Self> {
        let file = std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(O_NONBLOCK)
            .open(path)
            .ok()?;
        let mut keys = [0u8; KEY_MAX / 8 + 1];
        let mut axes = [0u8; ABS_COUNT.div_ceil(8)];
        if !ioctl_read(
            &file,
            eviocg(0x20 + u32::from(EV_KEY), keys.len()),
            &mut keys,
        ) || !ioctl_read(
            &file,
            eviocg(0x20 + u32::from(EV_ABS), axes.len()),
            &mut axes,
        ) || !bit(&keys, usize::from(BTN_SOUTH))
            || !bit(&axes, usize::from(ABS_X))
            || !bit(&axes, usize::from(ABS_Y))
        {
            return None;
        }
        let layout = Layout::from_axes(|code| bit(&axes, usize::from(code)));
        let mut pad = Self {
            path: path.to_owned(),
            file,
            decoder: Decoder::new(
                layout,
                Raw {
                    buttons: 0,
                    digital_triggers: [false; 2],
                    abs: [AbsInfo::default(); ABS_COUNT],
                },
            ),
        };
        let raw = pad.read_state()?;
        pad.decoder = Decoder::new(layout, raw);
        Some(pad)
    }

    fn read_state(&self) -> Option<Raw> {
        let mut raw = Raw {
            buttons: 0,
            digital_triggers: [false; 2],
            abs: [AbsInfo::default(); ABS_COUNT],
        };
        let mut axes = [0u8; ABS_COUNT.div_ceil(8)];
        if !ioctl_read(
            &self.file,
            eviocg(0x20 + u32::from(EV_ABS), axes.len()),
            &mut axes,
        ) {
            return None;
        }
        for code in 0..ABS_COUNT {
            if bit(&axes, code) {
                let mut info = [0u8; size_of::<AbsInfo>()];
                if !ioctl_read(
                    &self.file,
                    eviocg(0x40 + code as u32, info.len()),
                    &mut info,
                ) {
                    return None;
                }
                // SAFETY: AbsInfo is six plain i32s, valid for any bytes.
                raw.abs[code] = unsafe { std::ptr::read_unaligned(info.as_ptr().cast()) };
            }
        }
        let mut keys = [0u8; KEY_MAX / 8 + 1];
        if !ioctl_read(&self.file, eviocg(0x18, keys.len()), &mut keys) {
            return None;
        }
        for &(code, _) in &BUTTONS {
            raw.key(code, bit(&keys, usize::from(code)));
        }
        raw.key(BTN_TL2, bit(&keys, usize::from(BTN_TL2)));
        raw.key(BTN_TR2, bit(&keys, usize::from(BTN_TR2)));
        Some(raw)
    }

    fn pump(&mut self) -> Result<(), ()> {
        let mut buffer = [0u8; EVENT_SIZE * 64];
        loop {
            let length = match self.file.read(&mut buffer) {
                Ok(0) => return Err(()),
                Ok(length) => length,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) if error.raw_os_error() == Some(ENODEV) => return Err(()),
                Err(_) => return Err(()),
            };
            for event in buffer[..length].as_chunks::<EVENT_SIZE>().0 {
                let kind = u16::from_ne_bytes([event[16], event[17]]);
                let code = u16::from_ne_bytes([event[18], event[19]]);
                let value = i32::from_ne_bytes([event[20], event[21], event[22], event[23]]);
                if let Step::Resync = self.decoder.event(kind, code, value) {
                    let raw = self.read_state().ok_or(())?;
                    self.decoder.publish(raw);
                }
            }
        }
    }
}

struct Pads {
    slots: [Option<Pad>; 4],
    last_scan: Option<Instant>,
}

static PADS: Mutex<Pads> = Mutex::new(Pads {
    slots: [None, None, None, None],
    last_scan: None,
});

impl Pads {
    fn scan(&mut self) {
        let Ok(entries) = std::fs::read_dir("/dev/input") else {
            return;
        };
        let mut nodes: Vec<(u32, PathBuf)> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let number = entry
                    .file_name()
                    .to_str()?
                    .strip_prefix("event")?
                    .parse()
                    .ok()?;
                Some((number, entry.path()))
            })
            .collect();
        nodes.sort();
        for (_, path) in nodes {
            if self.slots.iter().flatten().any(|pad| pad.path == path) {
                continue;
            }
            let Some(slot) = self.slots.iter_mut().find(|slot| slot.is_none()) else {
                return;
            };
            if let Some(pad) = Pad::open(&path) {
                *slot = Some(pad);
            }
        }
    }
}

pub(super) fn poll(index: usize) -> Result<XboxState, DeviceError> {
    let mut pads = PADS.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if pads.slots[index].is_none() {
        let now = Instant::now();
        if pads
            .last_scan
            .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(1))
        {
            pads.last_scan = Some(now);
            pads.scan();
        }
    }
    let Some(pad) = pads.slots[index].as_mut() else {
        return Err(DeviceError::Disconnected);
    };
    if pad.pump().is_err() {
        pads.slots[index] = None;
        return Err(DeviceError::Disconnected);
    }
    Ok(pad.decoder.packet())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(minimum: i32, maximum: i32) -> AbsInfo {
        AbsInfo {
            minimum,
            maximum,
            ..AbsInfo::default()
        }
    }

    fn xpad() -> Raw {
        let mut abs = [AbsInfo::default(); ABS_COUNT];
        for code in [ABS_X, ABS_Y, ABS_RX, ABS_RY] {
            abs[usize::from(code)] = info(-32768, 32767);
        }
        for code in [ABS_Z, ABS_RZ] {
            abs[usize::from(code)] = info(0, 255);
        }
        for code in [ABS_HAT0X, ABS_HAT0Y] {
            abs[usize::from(code)] = info(-1, 1);
        }
        Raw {
            buttons: 0,
            digital_triggers: [false; 2],
            abs,
        }
    }

    #[test]
    fn xpad_axes_are_bit_identical_to_xinput() {
        let full = info(-32768, 32767);
        for value in [-32768, -1, 0, 1, 12345, 32767] {
            assert_eq!(stick(value, &full, false), value as i16);
            assert_eq!(stick(!value, &full, true), value as i16);
        }
        let byte = info(0, 255);
        for value in [0, 1, 128, 255] {
            assert_eq!(trigger(value, &byte), value as u8);
        }
    }

    #[test]
    fn other_ranges_rescale_onto_xinput_ranges() {
        let ds = info(0, 255);
        assert_eq!(stick(0, &ds, false), i16::MIN);
        assert_eq!(stick(255, &ds, false), i16::MAX);
        assert_eq!(stick(0, &ds, true), i16::MAX);
        let bt = info(0, 65535);
        assert_eq!(stick(0, &bt, false), i16::MIN);
        assert_eq!(stick(65535, &bt, false), i16::MAX);
        assert_eq!(stick(32768, &bt, false), 0);
        let wide = info(0, 1023);
        assert_eq!(trigger(0, &wide), 0);
        assert_eq!(trigger(1023, &wide), 255);
        assert_eq!(trigger(512, &wide), 128);
        assert_eq!(trigger(5000, &wide), 255);
    }

    #[test]
    fn layouts() {
        let has = |codes: &[u16]| Layout::from_axes(|code| codes.contains(&code));
        assert_eq!(
            has(&[ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ]),
            Layout {
                right: [ABS_RX, ABS_RY],
                triggers: Some([ABS_Z, ABS_RZ])
            }
        );
        assert_eq!(
            has(&[ABS_X, ABS_Y, ABS_Z, ABS_RZ, ABS_GAS, ABS_BRAKE]),
            Layout {
                right: [ABS_Z, ABS_RZ],
                triggers: Some([ABS_BRAKE, ABS_GAS])
            }
        );
        assert_eq!(
            has(&[ABS_X, ABS_Y, ABS_RX, ABS_RY]),
            Layout {
                right: [ABS_RX, ABS_RY],
                triggers: None
            }
        );
    }

    #[test]
    fn events_publish_on_syn_report() {
        let layout =
            Layout::from_axes(|code| [ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ].contains(&code));
        let mut decoder = Decoder::new(layout, xpad());
        let before = decoder.packet();
        decoder.event(EV_KEY, 0x130, 1);
        decoder.event(EV_ABS, ABS_RZ, 200);
        decoder.event(EV_ABS, ABS_Y, !1000);
        assert_eq!(decoder.packet(), before);
        decoder.event(EV_SYN, SYN_REPORT, 0);
        let packet = decoder.packet();
        assert_eq!(packet.buttons, 0x1000);
        assert_eq!(packet.triggers, [0, 200]);
        assert_eq!(packet.left, [0, 1000]);
        decoder.event(EV_ABS, ABS_HAT0X, -1);
        decoder.event(EV_ABS, ABS_HAT0Y, 1);
        decoder.event(EV_KEY, 0x130, 0);
        decoder.event(EV_SYN, SYN_REPORT, 0);
        assert_eq!(decoder.packet().buttons, DPAD_LEFT | DPAD_DOWN);
    }

    #[test]
    fn syn_dropped_discards_until_report_then_resyncs() {
        let layout =
            Layout::from_axes(|code| [ABS_X, ABS_Y, ABS_Z, ABS_RX, ABS_RY, ABS_RZ].contains(&code));
        let mut decoder = Decoder::new(layout, xpad());
        decoder.event(EV_SYN, SYN_DROPPED, 0);
        decoder.event(EV_KEY, 0x130, 1);
        assert!(matches!(decoder.event(EV_SYN, SYN_REPORT, 0), Step::Resync));
        assert_eq!(decoder.packet().buttons, 0);
        let mut raw = xpad();
        raw.key(0x130, true);
        decoder.publish(raw);
        assert_eq!(decoder.packet().buttons, 0x1000);
    }

    #[test]
    fn digital_triggers_read_as_full_pull() {
        let layout = Layout::from_axes(|code| [ABS_X, ABS_Y, ABS_RX, ABS_RY].contains(&code));
        let mut decoder = Decoder::new(layout, xpad());
        decoder.event(EV_KEY, BTN_TR2, 1);
        decoder.event(EV_SYN, SYN_REPORT, 0);
        assert_eq!(decoder.packet().triggers, [0, 255]);
    }

    #[test]
    fn ioctl_numbers_match_the_kernel_headers() {
        assert_eq!(eviocg(0x20 + 1, 96), 0x8060_4521);
        assert_eq!(eviocg(0x40, 24), 0x8018_4540);
        assert_eq!(eviocg(0x18, 96), 0x8060_4518);
    }
}
