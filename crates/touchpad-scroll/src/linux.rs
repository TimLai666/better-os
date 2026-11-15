//! The real nodes: evdev in, uinput out. The only module that talks to the
//! kernel, and a thin one: every decision is made in the modules it serves.
//!
//! Nothing in the test suite constructs these types. They open the node they
//! are given and nothing else; which node that is was decided by the unit (one
//! instance per node, `DeviceAllow=` limiting it to that node and
//! `/dev/uinput`) and is checked again with [`crate::caps::classify`] before
//! anything is grabbed.

#![allow(unsafe_code)]

use std::ffi::CString;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use crate::caps::{AbsInfo, Bits, DeviceDescriptor, InputId, Refusal, clone_descriptor};
use crate::codes::*;
use crate::event::InputEvent;
use crate::passthrough::{Sink, SinkFactory, Source};

#[cfg(not(target_pointer_width = "64"))]
compile_error!("the input_event layout below is the 64-bit one");

const IOC_NONE: u64 = 0;
const IOC_WRITE: u64 = 1;
const IOC_READ: u64 = 2;

const fn ioc(direction: u64, kind: u8, number: u8, size: usize) -> u64 {
    (direction << 30) | ((size as u64) << 16) | ((kind as u64) << 8) | number as u64
}

const fn eviocgname(len: usize) -> u64 {
    ioc(IOC_READ, b'E', 0x06, len)
}
const fn eviocgphys(len: usize) -> u64 {
    ioc(IOC_READ, b'E', 0x07, len)
}
const fn eviocgprop(len: usize) -> u64 {
    ioc(IOC_READ, b'E', 0x09, len)
}
const fn eviocgkey(len: usize) -> u64 {
    ioc(IOC_READ, b'E', 0x18, len)
}
const fn eviocgbit(event: u16, len: usize) -> u64 {
    ioc(IOC_READ, b'E', 0x20 + event as u8, len)
}
const fn eviocgabs(axis: u16) -> u64 {
    ioc(IOC_READ, b'E', 0x40 + axis as u8, size_of::<RawAbsInfo>())
}
const EVIOCGID: u64 = ioc(IOC_READ, b'E', 0x02, size_of::<RawInputId>());
const EVIOCGRAB: u64 = ioc(IOC_WRITE, b'E', 0x90, size_of::<libc::c_int>());

const UI_DEV_CREATE: u64 = ioc(IOC_NONE, b'U', 1, 0);
const UI_DEV_DESTROY: u64 = ioc(IOC_NONE, b'U', 2, 0);
const UI_DEV_SETUP: u64 = ioc(IOC_WRITE, b'U', 3, size_of::<UinputSetup>());
const UI_ABS_SETUP: u64 = ioc(IOC_WRITE, b'U', 4, size_of::<UinputAbsSetup>());
const UI_SET_EVBIT: u64 = ioc(IOC_WRITE, b'U', 100, size_of::<libc::c_int>());
const UI_SET_KEYBIT: u64 = ioc(IOC_WRITE, b'U', 101, size_of::<libc::c_int>());
const UI_SET_RELBIT: u64 = ioc(IOC_WRITE, b'U', 102, size_of::<libc::c_int>());
const UI_SET_ABSBIT: u64 = ioc(IOC_WRITE, b'U', 103, size_of::<libc::c_int>());
const UI_SET_MSCBIT: u64 = ioc(IOC_WRITE, b'U', 104, size_of::<libc::c_int>());
const UI_SET_PHYS: u64 = ioc(IOC_WRITE, b'U', 108, size_of::<*const libc::c_char>());
const UI_SET_PROPBIT: u64 = ioc(IOC_WRITE, b'U', 110, size_of::<libc::c_int>());

/// The major number of every input device node.
const INPUT_MAJOR: u32 = 13;
/// `/dev/uinput`'s numbers.
const MISC_MAJOR: u32 = 10;
const UINPUT_MINOR: u32 = 223;

#[repr(C)]
#[derive(Default)]
struct RawInputId {
    bustype: u16,
    vendor: u16,
    product: u16,
    version: u16,
}

#[repr(C)]
#[derive(Default, Clone, Copy)]
struct RawAbsInfo {
    value: i32,
    minimum: i32,
    maximum: i32,
    fuzz: i32,
    flat: i32,
    resolution: i32,
}

#[repr(C)]
struct UinputSetup {
    id: RawInputId,
    name: [u8; 80],
    ff_effects_max: u32,
}

#[repr(C)]
struct UinputAbsSetup {
    code: u16,
    absinfo: RawAbsInfo,
}

/// `struct input_event` on a 64-bit kernel.
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct RawEvent {
    seconds: i64,
    microseconds: i64,
    kind: u16,
    code: u16,
    value: i32,
}

const _: () = assert!(size_of::<RawEvent>() == 24);
const _: () = assert!(size_of::<UinputSetup>() == 92);
const _: () = assert!(size_of::<UinputAbsSetup>() == 28);

fn check(result: libc::c_int) -> io::Result<libc::c_int> {
    if result < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(result)
    }
}

/// `ioctl` with a pointer argument.
///
/// # Safety
/// `argument` must point at memory of the size `request` encodes.
unsafe fn ioctl_ptr<T>(fd: RawFd, request: u64, argument: *mut T) -> io::Result<libc::c_int> {
    // SAFETY: forwarded from the caller.
    check(unsafe { libc::ioctl(fd, request as _, argument) })
}

fn ioctl_int(fd: RawFd, request: u64, argument: libc::c_int) -> io::Result<libc::c_int> {
    // SAFETY: the request takes an integer by value.
    check(unsafe { libc::ioctl(fd, request as _, argument) })
}

fn bits(fd: RawFd, request: u64, len: usize) -> io::Result<Bits> {
    let mut buffer = vec![0u8; len];
    // SAFETY: the buffer is `len` bytes, the size the request encodes.
    let read = unsafe { ioctl_ptr(fd, request, buffer.as_mut_ptr())? };
    buffer.truncate(read as usize);
    Ok(Bits::from_le_bytes(&buffer))
}

fn string(fd: RawFd, request: u64) -> io::Result<String> {
    let mut buffer = [0u8; 256];
    // SAFETY: the buffer is 256 bytes, the size the request encodes.
    unsafe { ioctl_ptr(fd, request, buffer.as_mut_ptr())? };
    let end = buffer
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(buffer.len());
    Ok(String::from_utf8_lossy(&buffer[..end]).into_owned())
}

fn open_char_device(
    path: &Path,
    flags: libc::c_int,
    major: u32,
    minor: Option<u32>,
) -> io::Result<(OwnedFd, u32)> {
    use std::os::unix::ffi::OsStrExt;
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
    // SAFETY: a valid C string and plain flags.
    let fd =
        check(unsafe { libc::open(path.as_ptr(), flags | libc::O_CLOEXEC | libc::O_NOFOLLOW) })?;
    // SAFETY: `open` returned a new descriptor this function now owns.
    let fd = unsafe { OwnedFd::from_raw_fd(fd) };
    // SAFETY: a zeroed stat is a valid out-parameter.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: `fd` is open and `stat` is a valid out-parameter.
    check(unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) })?;
    let is_char = stat.st_mode & libc::S_IFMT == libc::S_IFCHR;
    let device_major = libc::major(stat.st_rdev);
    let device_minor = libc::minor(stat.st_rdev);
    if !is_char || device_major != major || minor.is_some_and(|minor| minor != device_minor) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the path is not the character device it was expected to be",
        ));
    }
    Ok((fd, device_minor))
}

pub struct EvdevSource {
    fd: OwnedFd,
    descriptor: DeviceDescriptor,
    buffer: Vec<RawEvent>,
}

impl EvdevSource {
    /// Open an input event node read-only. Refused unless it really is one.
    pub fn open(path: &Path) -> io::Result<Self> {
        let (fd, minor) =
            open_char_device(path, libc::O_RDONLY | libc::O_NONBLOCK, INPUT_MAJOR, None)?;
        let mut descriptor = describe(fd.as_raw_fd())?;
        // Read while the service may still read /run/udev; see crate::udev.
        descriptor.udev = crate::udev::read_db(INPUT_MAJOR, minor);
        Ok(Self {
            fd,
            descriptor,
            buffer: vec![RawEvent::default(); 64],
        })
    }

    fn read_raw(&mut self) -> io::Result<usize> {
        let bytes = self.buffer.len() * size_of::<RawEvent>();
        // SAFETY: the buffer holds `bytes` bytes of plain data.
        let read =
            unsafe { libc::read(self.fd.as_raw_fd(), self.buffer.as_mut_ptr().cast(), bytes) };
        if read < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(read as usize / size_of::<RawEvent>())
    }
}

fn describe(fd: RawFd) -> io::Result<DeviceDescriptor> {
    let mut id = RawInputId::default();
    // SAFETY: `id` is the struct EVIOCGID fills.
    unsafe { ioctl_ptr(fd, EVIOCGID, &mut id)? };
    let absolute = bits(fd, eviocgbit(EV_ABS, 8), 8)?;
    let mut abs_info = std::collections::BTreeMap::new();
    for axis in absolute.iter() {
        let mut info = RawAbsInfo::default();
        // SAFETY: `info` is the struct EVIOCGABS fills.
        unsafe { ioctl_ptr(fd, eviocgabs(axis), &mut info)? };
        abs_info.insert(
            axis,
            AbsInfo {
                value: info.value,
                minimum: info.minimum,
                maximum: info.maximum,
                fuzz: info.fuzz,
                flat: info.flat,
                resolution: info.resolution,
            },
        );
    }
    Ok(DeviceDescriptor {
        name: string(fd, eviocgname(256))?,
        phys: string(fd, eviocgphys(256)).unwrap_or_default(),
        id: InputId {
            bustype: id.bustype,
            vendor: id.vendor,
            product: id.product,
            version: id.version,
        },
        properties: bits(fd, eviocgprop(4), 4)?,
        events: bits(fd, eviocgbit(0, 4), 4)?,
        keys: bits(fd, eviocgbit(EV_KEY, 96), 96)?,
        relative: bits(fd, eviocgbit(EV_REL, 2), 2)?,
        absolute,
        misc: bits(fd, eviocgbit(EV_MSC, 1), 1)?,
        abs_info,
        udev: Default::default(),
    })
}

impl Source for EvdevSource {
    fn descriptor(&self) -> &DeviceDescriptor {
        &self.descriptor
    }

    fn set_grab(&mut self, grab: bool) -> io::Result<()> {
        ioctl_int(self.fd.as_raw_fd(), EVIOCGRAB, libc::c_int::from(grab)).map(|_| ())
    }

    fn read(&mut self, timeout: Duration, out: &mut Vec<InputEvent>) -> io::Result<()> {
        let mut poll = libc::pollfd {
            fd: self.fd.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let millis = timeout.as_millis().min(i32::MAX as u128) as libc::c_int;
        // SAFETY: one valid pollfd.
        let ready = unsafe { libc::poll(&mut poll, 1, millis) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::Interrupted {
                Ok(())
            } else {
                Err(error)
            };
        }
        if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
            return Err(io::Error::from_raw_os_error(libc::ENODEV));
        }
        if ready == 0 {
            return Ok(());
        }
        loop {
            match self.read_raw() {
                Ok(0) => return Ok(()),
                Ok(count) => {
                    out.extend(
                        self.buffer[..count]
                            .iter()
                            .map(|raw| InputEvent::new(raw.kind, raw.code, raw.value)),
                    );
                    if count < self.buffer.len() {
                        return Ok(());
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(error) => return Err(error),
            }
        }
    }

    fn drain(&mut self) -> io::Result<usize> {
        let mut drained = 0;
        loop {
            match self.read_raw() {
                Ok(0) => return Ok(drained),
                Ok(count) => drained += count,
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(drained),
                Err(error) => return Err(error),
            }
        }
    }

    fn touching(&mut self) -> io::Result<bool> {
        let keys = bits(self.fd.as_raw_fd(), eviocgkey(96), 96)?;
        Ok([BTN_TOUCH, BTN_LEFT, BTN_RIGHT, BTN_MIDDLE]
            .iter()
            .chain(FINGER_TOOLS.iter())
            .any(|code| keys.has(*code)))
    }

    fn current_slot(&mut self) -> io::Result<i32> {
        if !self.descriptor.absolute.has(ABS_MT_SLOT) {
            return Ok(0);
        }
        let mut info = RawAbsInfo::default();
        // SAFETY: `info` is the struct EVIOCGABS fills.
        unsafe { ioctl_ptr(self.fd.as_raw_fd(), eviocgabs(ABS_MT_SLOT), &mut info)? };
        Ok(info.value)
    }
}

/// `/dev/uinput`, opened once while the service still may, and reused for
/// every clone it makes afterwards.
pub struct UinputFactory {
    fd: Rc<OwnedFd>,
}

impl UinputFactory {
    pub fn open(path: &Path) -> io::Result<Self> {
        let (fd, _) = open_char_device(
            path,
            libc::O_WRONLY | libc::O_NONBLOCK,
            MISC_MAJOR,
            Some(UINPUT_MINOR),
        )?;
        Ok(Self { fd: Rc::new(fd) })
    }
}

fn refused(refusal: Refusal) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, refusal.to_string())
}

impl SinkFactory for UinputFactory {
    type Sink = UinputSink;

    fn create(&mut self, device: &DeviceDescriptor) -> io::Result<UinputSink> {
        // What the clone may report is the source's capabilities, re-judged:
        // no key outside the button block, so the kernel itself drops any key
        // event written to the clone.
        let clone = clone_descriptor(device).map_err(refused)?;
        let fd = self.fd.as_raw_fd();
        for kind in clone.events.iter() {
            ioctl_int(fd, UI_SET_EVBIT, kind.into())?;
        }
        for (request, set) in [
            (UI_SET_KEYBIT, &clone.keys),
            (UI_SET_RELBIT, &clone.relative),
            (UI_SET_ABSBIT, &clone.absolute),
            (UI_SET_MSCBIT, &clone.misc),
            (UI_SET_PROPBIT, &clone.properties),
        ] {
            for code in set.iter() {
                ioctl_int(fd, request, code.into())?;
            }
        }
        let phys = CString::new(clone.phys.clone())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        // SAFETY: UI_SET_PHYS takes a pointer to a C string, which the kernel
        // copies before returning.
        unsafe { check(libc::ioctl(fd, UI_SET_PHYS as _, phys.as_ptr()))? };
        for (code, info) in &clone.abs_info {
            let mut setup = UinputAbsSetup {
                code: *code,
                absinfo: RawAbsInfo {
                    value: info.value,
                    minimum: info.minimum,
                    maximum: info.maximum,
                    fuzz: info.fuzz,
                    flat: info.flat,
                    resolution: info.resolution,
                },
            };
            // SAFETY: `setup` is the struct UI_ABS_SETUP reads.
            unsafe { ioctl_ptr(fd, UI_ABS_SETUP, &mut setup)? };
        }
        let mut setup = UinputSetup {
            id: RawInputId {
                bustype: clone.id.bustype,
                vendor: clone.id.vendor,
                product: clone.id.product,
                version: clone.id.version,
            },
            name: [0; 80],
            ff_effects_max: 0,
        };
        let name = clone.name.as_bytes();
        let length = name.len().min(79);
        setup.name[..length].copy_from_slice(&name[..length]);
        // SAFETY: `setup` is the struct UI_DEV_SETUP reads.
        unsafe { ioctl_ptr(fd, UI_DEV_SETUP, &mut setup)? };
        ioctl_int(fd, UI_DEV_CREATE, 0)?;
        Ok(UinputSink {
            fd: self.fd.clone(),
            buffer: Vec::new(),
        })
    }
}

pub struct UinputSink {
    fd: Rc<OwnedFd>,
    buffer: Vec<RawEvent>,
}

impl Sink for UinputSink {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        self.buffer.clear();
        self.buffer.extend(events.iter().map(|event| RawEvent {
            kind: event.kind,
            code: event.code,
            value: event.value,
            ..RawEvent::default()
        }));
        let bytes = self.buffer.len() * size_of::<RawEvent>();
        // SAFETY: the buffer holds `bytes` bytes of plain data.
        let written =
            unsafe { libc::write(self.fd.as_raw_fd(), self.buffer.as_ptr().cast(), bytes) };
        if written < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for UinputSink {
    fn drop(&mut self) {
        let _ = ioctl_int(self.fd.as_raw_fd(), UI_DEV_DESTROY, 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_request_numbers_are_the_kernels() {
        // Values from <linux/input.h> and <linux/uinput.h> on x86_64 and arm64.
        assert_eq!(EVIOCGRAB, 0x4004_4590);
        assert_eq!(EVIOCGID, 0x8008_4502);
        assert_eq!(eviocgabs(ABS_MT_SLOT), 0x8018_456f);
        assert_eq!(eviocgbit(EV_KEY, 96), 0x8060_4521);
        assert_eq!(eviocgkey(96), 0x8060_4518);
        assert_eq!(UI_DEV_CREATE, 0x5501);
        assert_eq!(UI_DEV_DESTROY, 0x5502);
        assert_eq!(UI_DEV_SETUP, 0x405c_5503);
        assert_eq!(UI_ABS_SETUP, 0x401c_5504);
        assert_eq!(UI_SET_EVBIT, 0x4004_5564);
        assert_eq!(UI_SET_PHYS, 0x4008_556c);
        assert_eq!(UI_SET_PROPBIT, 0x4004_556e);
    }

    #[test]
    fn a_path_that_is_not_an_input_node_is_refused_before_any_ioctl() {
        // A regular file, opened by the same function the service uses.
        let file = tempfile::NamedTempFile::new().unwrap();
        let error = EvdevSource::open(file.path()).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        // /dev/null is a character device, but not an input one.
        let error = EvdevSource::open(Path::new("/dev/null")).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        let error = UinputFactory::open(Path::new("/dev/null")).err().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
}
