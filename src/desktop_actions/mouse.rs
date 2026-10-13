//! Linux relative-wheel device. Tests replace the ioctl/write boundary.
//!
//! The only unsafe operation is a scalar-argument libc ioctl. Record encoding
//! uses initialized byte arrays, never reads struct padding or raw pointers.

use super::ScrollDirection;
use std::{
    fs::{File, OpenOptions},
    io::{self, Write},
    os::fd::AsRawFd,
};

const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const BTN_LEFT: u16 = 272;
const REL_HWHEEL: u16 = 6;
const REL_WHEEL: u16 = 8;
const EVENT_SIZE: usize = std::mem::size_of::<libc::input_event>();
const VALUE_OFFSET: usize = EVENT_SIZE - 8;

const CREATE: libc::Ioctl = libc::_IO(b'U' as u32, 1);
const DESTROY: libc::Ioctl = libc::_IO(b'U' as u32, 2);
const EVBIT: libc::Ioctl = libc::_IOW::<libc::c_int>(b'U' as u32, 100);
const KEYBIT: libc::Ioctl = libc::_IOW::<libc::c_int>(b'U' as u32, 101);
const RELBIT: libc::Ioctl = libc::_IOW::<libc::c_int>(b'U' as u32, 102);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Operation {
    Event(u16),
    Key(u16),
    Relative(u16),
    Create,
    Ready,
    Destroy,
}

pub(super) trait Backend: Write {
    fn configure(&mut self, operation: Operation) -> io::Result<()>;
}

pub(super) struct Device<B: Backend> {
    backend: B,
    created: bool,
}

impl<B: Backend> Device<B> {
    pub(super) fn new(backend: B) -> io::Result<Self> {
        let mut device = Self {
            backend,
            created: false,
        };
        device.backend.configure(Operation::Event(EV_KEY))?;
        device.backend.configure(Operation::Key(BTN_LEFT))?;
        device.backend.configure(Operation::Event(EV_REL))?;
        device.backend.configure(Operation::Relative(REL_WHEEL))?;
        device.backend.configure(Operation::Relative(REL_HWHEEL))?;
        // Legacy uinput_user_dev has no pointer-sized fields: name[80],
        // input_id[8], ff_effects_max[4], then four int[64] axis arrays.
        // All axis fields are zero because this device is exclusively relative.
        let mut setup = [0_u8; 1116];
        let name = b"Stream Deck relative wheel";
        setup[..name.len()].copy_from_slice(name);
        setup[80..82].copy_from_slice(&3_u16.to_ne_bytes()); // BUS_USB
        setup[82..84].copy_from_slice(&0x0fd9_u16.to_ne_bytes());
        setup[84..86].copy_from_slice(&0x0084_u16.to_ne_bytes());
        setup[86..88].copy_from_slice(&1_u16.to_ne_bytes());
        device.backend.write_all(&setup)?;
        device.backend.configure(Operation::Create)?;
        device.created = true;
        device.backend.configure(Operation::Ready)?;
        Ok(device)
    }

    pub(super) fn scroll(&mut self, direction: ScrollDirection, ticks: u16) -> io::Result<()> {
        let (code, sign): (u16, i32) = match direction {
            ScrollDirection::Up => (REL_WHEEL, 1),
            ScrollDirection::Down => (REL_WHEEL, -1),
            ScrollDirection::Left => (REL_HWHEEL, -1),
            ScrollDirection::Right => (REL_HWHEEL, 1),
        };
        let value = i32::from(ticks) * sign; // u16 magnitude fits signed i32.
        let mut frame = [0_u8; EVENT_SIZE * 2];
        frame[VALUE_OFFSET..VALUE_OFFSET + 2].copy_from_slice(&EV_REL.to_ne_bytes());
        frame[VALUE_OFFSET + 2..VALUE_OFFSET + 4].copy_from_slice(&code.to_ne_bytes());
        frame[VALUE_OFFSET + 4..EVENT_SIZE].copy_from_slice(&value.to_ne_bytes());
        // The second all-zero record is EV_SYN/SYN_REPORT.
        write_records(&mut self.backend, &frame)
    }
}

impl<B: Backend> Drop for Device<B> {
    fn drop(&mut self) {
        if self.created {
            if let Err(error) = self.backend.configure(Operation::Destroy) {
                eprintln!("Cannot destroy Stream Deck relative wheel: {error}");
            }
        }
        // Closing Native's File also destroys the kernel device.
    }
}

fn write_records(writer: &mut impl Write, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        match writer.write(bytes) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
            Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
            Ok(count) if count > bytes.len() || count % EVENT_SIZE != 0 => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "split uinput event record",
                ));
            }
            Ok(count) => bytes = &bytes[count..],
        }
    }
    Ok(())
}

pub(super) struct Native(File);

impl Native {
    pub(super) fn open() -> io::Result<Self> {
        OpenOptions::new()
            .write(true)
            .open("/dev/uinput")
            .map(Self)
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("Cannot open mouse /dev/uinput: {error}"),
                )
            })
    }
}

impl Write for Native {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0.write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl Backend for Native {
    fn configure(&mut self, operation: Operation) -> io::Result<()> {
        if operation == Operation::Ready {
            // Match the existing physical keyboard's bounded hotplug grace.
            // This is not an acknowledgement from the compositor.
            std::thread::sleep(std::time::Duration::from_millis(200));
            return Ok(());
        }
        let (request, argument) = match operation {
            Operation::Event(value) => (EVBIT, libc::c_int::from(value)),
            Operation::Key(value) => (KEYBIT, libc::c_int::from(value)),
            Operation::Relative(value) => (RELBIT, libc::c_int::from(value)),
            Operation::Create => (CREATE, 0),
            Operation::Destroy => (DESTROY, 0),
            Operation::Ready => unreachable!("ready handled before ioctl"),
        };
        // SAFETY: [FFI boundary] File owns a live fd. These linux/uinput.h
        // requests take scalar c_int values or no argument, never pointers.
        // The syscall cannot read or mutate Rust memory through this argument.
        let result = unsafe { libc::ioctl(self.0.as_raw_fd(), request, argument) };
        if result < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};

    #[derive(Default)]
    struct State {
        operations: Vec<Operation>,
        writes: Vec<Vec<u8>>,
        fail_operation: Option<Operation>,
        split_write: bool,
    }
    struct Fake(Rc<RefCell<State>>);
    impl Write for Fake {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let mut state = self.0.borrow_mut();
            state.writes.push(bytes.to_vec());
            Ok(if state.split_write { 1 } else { bytes.len() })
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Backend for Fake {
        fn configure(&mut self, operation: Operation) -> io::Result<()> {
            let mut state = self.0.borrow_mut();
            state.operations.push(operation);
            if state.fail_operation == Some(operation) {
                Err(io::Error::other("ioctl failed"))
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn device_has_relative_wheel_capabilities_and_owned_teardown() {
        // Given an injectable uinput boundary.
        let state = Rc::new(RefCell::new(State::default()));
        // When a device is initialized and then dropped.
        let device = Device::new(Fake(state.clone())).unwrap();
        drop(device);
        // Then both axes and mouse classification are registered, and destroyed.
        assert_eq!(
            state.borrow().operations,
            [
                Operation::Event(EV_KEY),
                Operation::Key(BTN_LEFT),
                Operation::Event(EV_REL),
                Operation::Relative(REL_WHEEL),
                Operation::Relative(REL_HWHEEL),
                Operation::Create,
                Operation::Ready,
                Operation::Destroy,
            ]
        );
        assert_eq!(state.borrow().writes[0].len(), 1116);
    }

    #[test]
    fn setup_failure_prevents_device_use() {
        // Given a failed kernel capability boundary.
        let state = Rc::new(RefCell::new(State {
            fail_operation: Some(Operation::Relative(REL_HWHEEL)),
            ..State::default()
        }));
        // When initialization is attempted.
        assert!(Device::new(Fake(state.clone())).is_err());
        // Then no virtual device was created or input written.
        assert!(!state.borrow().operations.contains(&Operation::Create));
        assert!(state.borrow().writes.is_empty());
    }

    #[test]
    fn ready_failure_destroys_the_created_device() {
        // Given an initialization failure after creation.
        let state = Rc::new(RefCell::new(State {
            fail_operation: Some(Operation::Ready),
            ..State::default()
        }));
        // When initialization fails.
        assert!(Device::new(Fake(state.clone())).is_err());
        // Then created device ownership is torn down.
        assert_eq!(state.borrow().operations.last(), Some(&Operation::Destroy));
    }

    #[test]
    fn direction_and_tick_magnitude_encode_real_evdev_records() {
        // Given every wheel direction, including the maximum tick magnitude.
        for (direction, code, value) in [
            (ScrollDirection::Up, REL_WHEEL, 65535_i32),
            (ScrollDirection::Down, REL_WHEEL, -65535),
            (ScrollDirection::Left, REL_HWHEEL, -65535),
            (ScrollDirection::Right, REL_HWHEEL, 65535),
        ] {
            let state = Rc::new(RefCell::new(State::default()));
            let mut device = Device::new(Fake(state.clone())).unwrap();
            // When wheel input is emitted.
            device.scroll(direction, u16::MAX).unwrap();
            // Then native ABI records carry EV_REL and a trailing SYN_REPORT.
            let state = state.borrow();
            let bytes = state.writes.last().unwrap();
            assert_eq!(bytes.len(), EVENT_SIZE * 2);
            assert_eq!(
                &bytes[VALUE_OFFSET..VALUE_OFFSET + 2],
                &EV_REL.to_ne_bytes()
            );
            assert_eq!(
                &bytes[VALUE_OFFSET + 2..VALUE_OFFSET + 4],
                &code.to_ne_bytes()
            );
            assert_eq!(&bytes[VALUE_OFFSET + 4..EVENT_SIZE], &value.to_ne_bytes());
            assert!(bytes[..VALUE_OFFSET].iter().all(|byte| *byte == 0));
            assert!(bytes[EVENT_SIZE..].iter().all(|byte| *byte == 0));
        }
    }

    #[test]
    fn split_event_write_is_an_error() {
        // Given a device whose boundary splits a native input_event.
        let state = Rc::new(RefCell::new(State::default()));
        let mut device = Device::new(Fake(state.clone())).unwrap();
        state.borrow_mut().split_write = true;
        // When a wheel frame is emitted.
        let error = device.scroll(ScrollDirection::Down, 1).unwrap_err();
        // Then corruption cannot be accepted as successful input.
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
}
