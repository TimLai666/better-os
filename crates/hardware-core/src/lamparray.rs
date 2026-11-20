//! HID LampArray (HID Usage Tables, Lighting and Illumination page 0x59):
//! finding one in a report descriptor, reading what it says about its lamps,
//! and the three things Better OS asks of it (ticket 136, ADR 0034).
//!
//! No I/O. A device is anything that implements [`FeatureDevice`]: the
//! shipped one is the daemon's hidraw helper, and tests use
//! [`fake::FakeLampArray`]. Every byte that comes from a device (the report
//! descriptor and each feature report) is untrusted: the parser is bounded in
//! length, nesting, field count, and report size, and a report is read only
//! at offsets the descriptor itself declared and the reply is long enough to
//! hold.
//!
//! What is asked of a lamp array, and nothing else:
//! - its attributes (report LampArrayAttributes) and each lamp's (a
//!   LampAttributesRequest, then LampAttributesResponse);
//! - one colour and intensity for every lamp (LampRangeUpdate over the whole
//!   range, after LampArrayControl turned autonomous mode off);
//! - autonomous mode back on, which gives the lighting back to the firmware.
//!
//! Effects are not on this page of the usage tables: a LampArray takes
//! colours, and an animated effect would be a process rewriting them on a
//! timer, which Better OS does not do. Vendor reports (the M5406WA's 0x5F)
//! are never written; nothing here can build one.

use serde::{Deserialize, Serialize};

/// The usage page of lighting and illumination.
pub const LIGHTING_PAGE: u16 = 0x59;

/// Usages on [`LIGHTING_PAGE`], as the HID Usage Tables name them.
pub mod usage {
    pub const LAMP_ARRAY: u16 = 0x01;
    pub const ATTRIBUTES_REPORT: u16 = 0x02;
    pub const LAMP_COUNT: u16 = 0x03;
    pub const LAMP_ARRAY_KIND: u16 = 0x07;
    pub const MIN_UPDATE_INTERVAL: u16 = 0x08;
    pub const ATTRIBUTES_REQUEST_REPORT: u16 = 0x20;
    pub const LAMP_ID: u16 = 0x21;
    pub const ATTRIBUTES_RESPONSE_REPORT: u16 = 0x22;
    pub const RED_LEVEL_COUNT: u16 = 0x28;
    pub const GREEN_LEVEL_COUNT: u16 = 0x29;
    pub const BLUE_LEVEL_COUNT: u16 = 0x2A;
    pub const INTENSITY_LEVEL_COUNT: u16 = 0x2B;
    pub const IS_PROGRAMMABLE: u16 = 0x2C;
    pub const RED_CHANNEL: u16 = 0x51;
    pub const GREEN_CHANNEL: u16 = 0x52;
    pub const BLUE_CHANNEL: u16 = 0x53;
    pub const INTENSITY_CHANNEL: u16 = 0x54;
    pub const UPDATE_FLAGS: u16 = 0x55;
    pub const RANGE_UPDATE_REPORT: u16 = 0x60;
    pub const LAMP_ID_START: u16 = 0x61;
    pub const LAMP_ID_END: u16 = 0x62;
    pub const CONTROL_REPORT: u16 = 0x70;
    pub const AUTONOMOUS_MODE: u16 = 0x71;
}

/// The kernel's `HID_MAX_DESCRIPTOR_SIZE`: no hidraw descriptor is longer.
pub const MAX_DESCRIPTOR_LEN: usize = 4096;
/// More nested collections than any real device uses.
const MAX_COLLECTION_DEPTH: usize = 32;
/// More global-state pushes than any real device uses.
const MAX_PUSH_DEPTH: usize = 16;
/// More fields than a 4096-byte descriptor can sensibly declare.
const MAX_FIELDS: usize = 8192;
/// A feature report the kernel would refuse anyway (`HID_MAX_BUFFER_SIZE`).
pub const MAX_REPORT_LEN: usize = 4096;
/// The most lamps Better OS reads attributes for. A per-key keyboard has
/// about a hundred; a device claiming more is refused rather than read for
/// minutes.
pub const MAX_LAMPS: u16 = 1024;

/// Which kind of main item a field belongs to.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportKind {
    Input,
    Output,
    Feature,
}

/// One field of one report, as the descriptor declares it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Field {
    pub kind: ReportKind,
    pub report_id: u8,
    pub usage_page: u16,
    pub usage: u16,
    /// Bits from the start of the report's data, after the report ID byte.
    pub bit_offset: u32,
    pub bit_size: u32,
    pub logical_max: i64,
    /// Whether the field sits inside a LampArray application collection.
    pub in_lamp_array: bool,
    /// The usage of the innermost collection on the lighting page around it.
    pub collection: Option<u16>,
}

/// Why a descriptor could not be read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DescriptorError {
    Empty,
    TooLong(usize),
    /// An item's data runs past the end.
    Truncated {
        at: usize,
    },
    TooDeep,
    UnbalancedCollection {
        at: usize,
    },
    TooManyFields,
    BadPushPop {
        at: usize,
    },
    /// A report would be longer than any the kernel moves.
    ReportTooLong {
        report_id: u8,
    },
    BadReportSize {
        at: usize,
    },
}

impl std::fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DescriptorError::Empty => write!(f, "the report descriptor is empty"),
            DescriptorError::TooLong(n) => write!(f, "the report descriptor is {n} bytes"),
            DescriptorError::Truncated { at } => write!(f, "an item at byte {at} is cut short"),
            DescriptorError::TooDeep => write!(f, "collections nest too deeply"),
            DescriptorError::UnbalancedCollection { at } => {
                write!(f, "an end of collection at byte {at} closes nothing")
            }
            DescriptorError::TooManyFields => write!(f, "too many fields"),
            DescriptorError::BadPushPop { at } => write!(f, "a push or pop at byte {at}"),
            DescriptorError::ReportTooLong { report_id } => {
                write!(f, "report {report_id:#04x} is too long")
            }
            DescriptorError::BadReportSize { at } => {
                write!(f, "a report size or count at byte {at} is out of range")
            }
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Globals {
    usage_page: u16,
    logical_min: i64,
    logical_max: i64,
    report_size: u32,
    report_count: u32,
    report_id: u8,
}

/// The unsigned value of an item's data, little-endian.
fn unsigned(data: &[u8]) -> u32 {
    data.iter()
        .rev()
        .fold(0u32, |acc, byte| (acc << 8) | u32::from(*byte))
}

/// The signed value of an item's data, as logical minimum and maximum are.
fn signed(data: &[u8]) -> i64 {
    match data.len() {
        0 => 0,
        1 => i64::from(data[0] as i8),
        2 => i64::from(i16::from_le_bytes([data[0], data[1]])),
        _ => i64::from(i32::from_le_bytes([data[0], data[1], data[2], data[3]])),
    }
}

/// The fields a report descriptor declares. Every count and offset is
/// checked; a descriptor that would make a report longer than the kernel
/// moves, or that is malformed anywhere, is refused whole.
pub fn parse_descriptor(bytes: &[u8]) -> Result<Vec<Field>, DescriptorError> {
    if bytes.is_empty() {
        return Err(DescriptorError::Empty);
    }
    if bytes.len() > MAX_DESCRIPTOR_LEN {
        return Err(DescriptorError::TooLong(bytes.len()));
    }
    let mut fields = Vec::new();
    let mut globals = Globals::default();
    let mut pushed: Vec<Globals> = Vec::new();
    // Local state, cleared after every main item.
    let mut usages: Vec<(u16, u16)> = Vec::new();
    let mut usage_min: Option<u32> = None;
    // (page, usage, is application LampArray) for each open collection.
    let mut collections: Vec<(u16, u16)> = Vec::new();
    // Bits used so far, per (kind, report id).
    let mut offsets: Vec<((ReportKind, u8), u32)> = Vec::new();

    let mut at = 0usize;
    while at < bytes.len() {
        let prefix = bytes[at];
        if prefix == 0xFE {
            // A long item: size, tag, data. None is defined; skip it.
            let size = *bytes.get(at + 1).ok_or(DescriptorError::Truncated { at })? as usize;
            let end = at + 3 + size;
            if end > bytes.len() {
                return Err(DescriptorError::Truncated { at });
            }
            at = end;
            continue;
        }
        let size = match prefix & 0x03 {
            3 => 4,
            n => n as usize,
        };
        let data = bytes
            .get(at + 1..at + 1 + size)
            .ok_or(DescriptorError::Truncated { at })?;
        let kind = (prefix >> 2) & 0x03;
        let tag = prefix >> 4;
        match kind {
            // Main.
            0 => {
                let report = match tag {
                    0x8 => Some(ReportKind::Input),
                    0x9 => Some(ReportKind::Output),
                    0xB => Some(ReportKind::Feature),
                    _ => None,
                };
                if let Some(report) = report {
                    let count = globals.report_count;
                    let bits = globals.report_size;
                    let key = (report, globals.report_id);
                    let slot = match offsets.iter().position(|(k, _)| *k == key) {
                        Some(i) => i,
                        None => {
                            offsets.push((key, 0));
                            offsets.len() - 1
                        }
                    };
                    let start = offsets[slot].1;
                    let total = bits
                        .checked_mul(count)
                        .and_then(|b| b.checked_add(start))
                        .filter(|b| *b as usize <= (MAX_REPORT_LEN - 1) * 8)
                        .ok_or(DescriptorError::ReportTooLong {
                            report_id: globals.report_id,
                        })?;
                    offsets[slot].1 = total;
                    // Ranges are expanded only as far as the fields need.
                    let mut expanded = usages.clone();
                    if let Some(min) = usage_min
                        && expanded.is_empty()
                    {
                        let page = if min > 0xFFFF {
                            (min >> 16) as u16
                        } else {
                            globals.usage_page
                        };
                        for i in 0..count.min(1024) {
                            let u = (min & 0xFFFF).saturating_add(i);
                            if u > 0xFFFF {
                                break;
                            }
                            expanded.push((page, u as u16));
                        }
                    }
                    let lamp = collections.first().is_some_and(|(page, usage)| {
                        *page == LIGHTING_PAGE && *usage == usage::LAMP_ARRAY
                    });
                    let inner = collections
                        .iter()
                        .rev()
                        .find(|(page, _)| *page == LIGHTING_PAGE)
                        .map(|(_, usage)| *usage);
                    if !expanded.is_empty() {
                        for i in 0..count {
                            let (page, usage) = expanded[(i as usize).min(expanded.len() - 1)];
                            if fields.len() >= MAX_FIELDS {
                                return Err(DescriptorError::TooManyFields);
                            }
                            fields.push(Field {
                                kind: report,
                                report_id: globals.report_id,
                                usage_page: page,
                                usage,
                                bit_offset: start + i * bits,
                                bit_size: bits,
                                logical_max: globals.logical_max,
                                in_lamp_array: lamp,
                                collection: inner,
                            });
                        }
                    }
                } else if tag == 0xA {
                    if collections.len() >= MAX_COLLECTION_DEPTH {
                        return Err(DescriptorError::TooDeep);
                    }
                    let (page, usage) = usages.first().copied().unwrap_or((globals.usage_page, 0));
                    // Only an application collection at the top starts a
                    // LampArray; a nested one is recorded for its usage.
                    let application = data.first().copied().unwrap_or(0) == 0x01;
                    if collections.is_empty() && !application {
                        collections.push((0, 0));
                    } else {
                        collections.push((page, usage));
                    }
                } else if tag == 0xC && collections.pop().is_none() {
                    return Err(DescriptorError::UnbalancedCollection { at });
                }
                usages.clear();
                usage_min = None;
            }
            // Global.
            1 => match tag {
                0x0 => globals.usage_page = unsigned(data) as u16,
                0x1 => globals.logical_min = signed(data),
                0x2 => {
                    // A maximum below a non-negative minimum is unsigned.
                    let value = signed(data);
                    globals.logical_max = if value < globals.logical_min && globals.logical_min >= 0
                    {
                        i64::from(unsigned(data))
                    } else {
                        value
                    };
                }
                0x7 => {
                    let value = unsigned(data);
                    if value == 0 || value > 256 {
                        return Err(DescriptorError::BadReportSize { at });
                    }
                    globals.report_size = value;
                }
                0x8 => {
                    let value = unsigned(data);
                    if value == 0 || value > 0xFF {
                        return Err(DescriptorError::BadReportSize { at });
                    }
                    globals.report_id = value as u8;
                }
                0x9 => {
                    let value = unsigned(data);
                    if value > (MAX_REPORT_LEN as u32) * 8 {
                        return Err(DescriptorError::BadReportSize { at });
                    }
                    globals.report_count = value;
                }
                0xA => {
                    if pushed.len() >= MAX_PUSH_DEPTH {
                        return Err(DescriptorError::BadPushPop { at });
                    }
                    pushed.push(globals);
                }
                0xB => globals = pushed.pop().ok_or(DescriptorError::BadPushPop { at })?,
                _ => {}
            },
            // Local.
            2 => match tag {
                0x0 => {
                    let value = unsigned(data);
                    let entry = if size == 4 {
                        ((value >> 16) as u16, value as u16)
                    } else {
                        (globals.usage_page, value as u16)
                    };
                    if usages.len() < 1024 {
                        usages.push(entry);
                    }
                }
                0x1 => usage_min = Some(unsigned(data)),
                _ => {}
            },
            // Reserved item type: skip it.
            _ => {}
        }
        at += 1 + size;
    }
    if !collections.is_empty() {
        return Err(DescriptorError::UnbalancedCollection { at: bytes.len() });
    }
    Ok(fields)
}

/// Where one value sits in one feature report.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Locator {
    pub bit_offset: u32,
    pub bit_size: u32,
    pub logical_max: i64,
}

/// One feature report: its ID, its length with the ID byte, and its fields.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    pub id: u8,
    pub len: usize,
    fields: Vec<(u16, Locator)>,
}

impl Report {
    fn field(&self, usage: u16) -> Option<Locator> {
        self.fields
            .iter()
            .find(|(u, _)| *u == usage)
            .map(|(_, l)| *l)
    }

    fn need(&self, usage: u16) -> Result<Locator, LayoutError> {
        self.field(usage).ok_or(LayoutError::MissingField {
            report: self.id,
            usage,
        })
    }
}

/// Why a descriptor is not a LampArray Better OS can drive.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutError {
    Descriptor(DescriptorError),
    /// No LampArray application collection.
    NotALampArray,
    MissingReport(u16),
    MissingField {
        report: u8,
        usage: u16,
    },
    /// A field Better OS writes or reads as a number is wider than 32 bits.
    FieldTooWide {
        report: u8,
        usage: u16,
    },
    /// A report without a report ID cannot sit beside the others.
    NoReportId,
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayoutError::Descriptor(error) => error.fmt(f),
            LayoutError::NotALampArray => write!(f, "the device is not a LampArray"),
            LayoutError::MissingReport(usage) => {
                write!(f, "the LampArray has no report for usage {usage:#04x}")
            }
            LayoutError::MissingField { report, usage } => {
                write!(
                    f,
                    "report {report:#04x} has no field for usage {usage:#04x}"
                )
            }
            LayoutError::FieldTooWide { report, usage } => {
                write!(
                    f,
                    "report {report:#04x} declares usage {usage:#04x} wider than 32 bits"
                )
            }
            LayoutError::NoReportId => write!(f, "the LampArray's reports have no report ID"),
        }
    }
}

/// The five feature reports a LampArray needs to be driven.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Layout {
    pub attributes: Report,
    pub request: Report,
    pub response: Report,
    pub range_update: Report,
    pub control: Report,
}

/// What the usage tables put in each report, and which of those Better OS
/// needs.
const REPORTS: [(u16, &[u16], &[u16]); 5] = [
    (
        usage::ATTRIBUTES_REPORT,
        &[usage::LAMP_COUNT],
        &[usage::LAMP_ARRAY_KIND, usage::MIN_UPDATE_INTERVAL],
    ),
    (usage::ATTRIBUTES_REQUEST_REPORT, &[usage::LAMP_ID], &[]),
    (
        usage::ATTRIBUTES_RESPONSE_REPORT,
        &[
            usage::LAMP_ID,
            usage::RED_LEVEL_COUNT,
            usage::GREEN_LEVEL_COUNT,
            usage::BLUE_LEVEL_COUNT,
            usage::INTENSITY_LEVEL_COUNT,
            usage::IS_PROGRAMMABLE,
        ],
        &[],
    ),
    (
        usage::RANGE_UPDATE_REPORT,
        &[
            usage::UPDATE_FLAGS,
            usage::LAMP_ID_START,
            usage::LAMP_ID_END,
            usage::RED_CHANNEL,
            usage::GREEN_CHANNEL,
            usage::BLUE_CHANNEL,
            usage::INTENSITY_CHANNEL,
        ],
        &[],
    ),
    (usage::CONTROL_REPORT, &[usage::AUTONOMOUS_MODE], &[]),
];

impl Layout {
    /// The LampArray in a descriptor, or why there is none. Only fields on
    /// the lighting page, inside a LampArray application collection, in a
    /// feature report count; a vendor copy of the same reports on another
    /// page (the M5406WA has one on 0xFF59) is never used.
    pub fn from_descriptor(bytes: &[u8]) -> Result<Layout, LayoutError> {
        let fields = parse_descriptor(bytes).map_err(LayoutError::Descriptor)?;
        let lamp: Vec<&Field> = fields
            .iter()
            .filter(|f| {
                f.in_lamp_array && f.kind == ReportKind::Feature && f.usage_page == LIGHTING_PAGE
            })
            .collect();
        if lamp.is_empty() {
            return Err(LayoutError::NotALampArray);
        }
        let mut reports = Vec::new();
        for (collection, required, optional) in REPORTS {
            let inside: Vec<&&Field> = lamp
                .iter()
                .filter(|f| f.collection == Some(collection))
                .collect();
            let Some(first) = inside.first() else {
                return Err(LayoutError::MissingReport(collection));
            };
            let id = first.report_id;
            if id == 0 {
                return Err(LayoutError::NoReportId);
            }
            // The report's length is every field of that ID, whichever
            // collection declared it.
            let bits = fields
                .iter()
                .filter(|f| f.kind == ReportKind::Feature && f.report_id == id)
                .map(|f| f.bit_offset + f.bit_size)
                .max()
                .unwrap_or(0);
            let len = 1 + bits.div_ceil(8) as usize;
            let mut chosen = Vec::new();
            for usage in required.iter().chain(optional.iter()) {
                if let Some(field) = inside
                    .iter()
                    .find(|f| f.usage == *usage && f.report_id == id)
                {
                    if field.bit_size > 32 {
                        return Err(LayoutError::FieldTooWide {
                            report: id,
                            usage: *usage,
                        });
                    }
                    chosen.push((
                        *usage,
                        Locator {
                            bit_offset: field.bit_offset,
                            bit_size: field.bit_size,
                            logical_max: field.logical_max,
                        },
                    ));
                }
            }
            let report = Report {
                id,
                len,
                fields: chosen,
            };
            for usage in required {
                report.need(*usage)?;
            }
            reports.push(report);
        }
        let mut reports = reports.into_iter();
        let mut next = || reports.next().expect("five reports");
        Ok(Layout {
            attributes: next(),
            request: next(),
            response: next(),
            range_update: next(),
            control: next(),
        })
    }
}

/// Whether a descriptor declares a LampArray Better OS can drive. The
/// unprivileged half of detection: `report_descriptor` in sysfs is readable
/// by everyone.
pub fn is_lamp_array(descriptor: &[u8]) -> bool {
    Layout::from_descriptor(descriptor).is_ok()
}

/// Reads `bits` (at most 32) at `offset` from a report's data, little-endian
/// as HID packs them. `None` when the report is too short to hold it.
fn get_bits(data: &[u8], offset: u32, bits: u32) -> Option<u32> {
    if bits == 0 || bits > 32 {
        return None;
    }
    let end = offset.checked_add(bits)?;
    if end.div_ceil(8) as usize > data.len() {
        return None;
    }
    let mut value = 0u64;
    for i in 0..bits {
        let bit = offset + i;
        let byte = data[(bit / 8) as usize];
        if byte >> (bit % 8) & 1 == 1 {
            value |= 1 << i;
        }
    }
    Some(value as u32)
}

fn put_bits(data: &mut [u8], offset: u32, bits: u32, value: u32) {
    for i in 0..bits.min(32) {
        let bit = offset + i;
        let Some(byte) = data.get_mut((bit / 8) as usize) else {
            return;
        };
        if value >> i & 1 == 1 {
            *byte |= 1 << (bit % 8);
        } else {
            *byte &= !(1 << (bit % 8));
        }
    }
}

/// The largest value a field can carry: its logical maximum, bounded by its
/// width.
fn field_max(locator: &Locator) -> u32 {
    let width_max = if locator.bit_size >= 32 {
        u32::MAX
    } else {
        (1u32 << locator.bit_size) - 1
    };
    if locator.logical_max <= 0 {
        width_max
    } else {
        u32::try_from(locator.logical_max)
            .unwrap_or(u32::MAX)
            .min(width_max)
    }
}

/// A device that moves HID feature reports. The first byte of every buffer
/// is the report ID, as hidraw's `HIDIOCGFEATURE` and `HIDIOCSFEATURE` take
/// it.
pub trait FeatureDevice {
    /// Fills `report` (whose first byte is the ID asked for) and returns how
    /// many bytes the device returned.
    fn get_feature(&mut self, report: &mut [u8]) -> Result<usize, String>;
    fn set_feature(&mut self, report: &[u8]) -> Result<(), String>;
}

/// `LampArrayKind`, as the usage tables number it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LampArrayKind {
    Undefined,
    Keyboard,
    Mouse,
    GameController,
    Peripheral,
    Scene,
    Notification,
    Chassis,
    Wearable,
    Furniture,
    Art,
    Other,
}

impl LampArrayKind {
    pub fn from_value(value: u32) -> LampArrayKind {
        match value {
            0 => LampArrayKind::Undefined,
            1 => LampArrayKind::Keyboard,
            2 => LampArrayKind::Mouse,
            3 => LampArrayKind::GameController,
            4 => LampArrayKind::Peripheral,
            5 => LampArrayKind::Scene,
            6 => LampArrayKind::Notification,
            7 => LampArrayKind::Chassis,
            8 => LampArrayKind::Wearable,
            9 => LampArrayKind::Furniture,
            10 => LampArrayKind::Art,
            _ => LampArrayKind::Other,
        }
    }
}

/// Whether a built-in LampArray's kind lets it be called the keyboard: it
/// says keyboard, or says nothing (undefined, or no such field). Anything
/// else is reported for what it is (ADR 0034).
pub fn is_keyboard_kind(kind: Option<LampArrayKind>) -> bool {
    matches!(
        kind,
        None | Some(LampArrayKind::Keyboard) | Some(LampArrayKind::Undefined)
    )
}

/// How many levels each channel has, for the whole array: the fewest any
/// lamp reports, so a colour every lamp can show.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Levels {
    pub red: u32,
    pub green: u32,
    pub blue: u32,
    pub intensity: u32,
}

impl Levels {
    /// Every colour channel can be set: a count of zero is a channel the
    /// lamp does not have (HID Usage Tables 1.4, section 26.9).
    pub fn has_colour(&self) -> bool {
        self.red > 0 && self.green > 0 && self.blue > 0
    }

    /// Intensity can be set separately from colour.
    pub fn has_intensity(&self) -> bool {
        self.intensity > 1
    }
}

/// What a LampArray says about itself.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Description {
    pub lamps: u16,
    /// `None` when the attributes report does not carry the field.
    pub kind: Option<LampArrayKind>,
    pub min_update_interval_us: Option<u32>,
    pub levels: Levels,
    /// Every lamp is programmable.
    pub programmable: bool,
}

/// Why a conversation with a LampArray failed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LampError {
    Device(String),
    /// The reply is shorter than the descriptor said, or carries another ID.
    BadReply {
        report: u8,
        got: usize,
    },
    NoLamps,
    TooManyLamps(u16),
    /// A lamp's attributes came back for another lamp.
    WrongLamp {
        asked: u16,
        got: u32,
    },
    NotProgrammable,
}

impl std::fmt::Display for LampError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LampError::Device(error) => write!(f, "{error}"),
            LampError::BadReply { report, got } => {
                write!(
                    f,
                    "report {report:#04x} came back with {got} bytes or another ID"
                )
            }
            LampError::NoLamps => write!(f, "the LampArray reports no lamps"),
            LampError::TooManyLamps(n) => write!(f, "the LampArray reports {n} lamps"),
            LampError::WrongLamp { asked, got } => {
                write!(f, "lamp {asked} was asked for and lamp {got} answered")
            }
            LampError::NotProgrammable => write!(f, "the lamps are not programmable"),
        }
    }
}

fn read_report(device: &mut dyn FeatureDevice, report: &Report) -> Result<Vec<u8>, LampError> {
    let mut buffer = vec![0u8; report.len];
    buffer[0] = report.id;
    let got = device.get_feature(&mut buffer).map_err(LampError::Device)?;
    if got < report.len || buffer[0] != report.id {
        return Err(LampError::BadReply {
            report: report.id,
            got,
        });
    }
    Ok(buffer)
}

fn value(data: &[u8], report: &Report, usage: u16) -> Result<u32, LampError> {
    let locator = report.need(usage).map_err(|_| LampError::BadReply {
        report: report.id,
        got: data.len(),
    })?;
    get_bits(&data[1..], locator.bit_offset, locator.bit_size).ok_or(LampError::BadReply {
        report: report.id,
        got: data.len(),
    })
}

fn optional(data: &[u8], report: &Report, usage: u16) -> Option<u32> {
    let locator = report.field(usage)?;
    get_bits(&data[1..], locator.bit_offset, locator.bit_size)
}

/// A report filled with `values`, each clamped to what its field carries.
fn build(report: &Report, values: &[(u16, u32)]) -> Vec<u8> {
    let mut buffer = vec![0u8; report.len];
    buffer[0] = report.id;
    for (usage, v) in values {
        if let Some(locator) = report.field(*usage) {
            put_bits(
                &mut buffer[1..],
                locator.bit_offset,
                locator.bit_size,
                (*v).min(field_max(&locator)),
            );
        }
    }
    buffer
}

/// Reads the array's attributes and every lamp's.
pub fn describe(device: &mut dyn FeatureDevice, layout: &Layout) -> Result<Description, LampError> {
    let attributes = read_report(device, &layout.attributes)?;
    let lamps = value(&attributes, &layout.attributes, usage::LAMP_COUNT)?;
    let lamps = u16::try_from(lamps).map_err(|_| LampError::TooManyLamps(u16::MAX))?;
    if lamps == 0 {
        return Err(LampError::NoLamps);
    }
    if lamps > MAX_LAMPS {
        return Err(LampError::TooManyLamps(lamps));
    }
    let kind = optional(&attributes, &layout.attributes, usage::LAMP_ARRAY_KIND)
        .map(LampArrayKind::from_value);
    let min_update_interval_us =
        optional(&attributes, &layout.attributes, usage::MIN_UPDATE_INTERVAL);
    let mut levels: Option<Levels> = None;
    let mut programmable = true;
    for lamp in 0..lamps {
        let request = build(&layout.request, &[(usage::LAMP_ID, u32::from(lamp))]);
        device.set_feature(&request).map_err(LampError::Device)?;
        let response = read_report(device, &layout.response)?;
        let id = value(&response, &layout.response, usage::LAMP_ID)?;
        if id != u32::from(lamp) {
            return Err(LampError::WrongLamp {
                asked: lamp,
                got: id,
            });
        }
        let read = |usage| value(&response, &layout.response, usage);
        let these = Levels {
            red: read(usage::RED_LEVEL_COUNT)?,
            green: read(usage::GREEN_LEVEL_COUNT)?,
            blue: read(usage::BLUE_LEVEL_COUNT)?,
            intensity: read(usage::INTENSITY_LEVEL_COUNT)?,
        };
        programmable &= read(usage::IS_PROGRAMMABLE)? != 0;
        levels = Some(match levels {
            None => these,
            Some(l) => Levels {
                red: l.red.min(these.red),
                green: l.green.min(these.green),
                blue: l.blue.min(these.blue),
                intensity: l.intensity.min(these.intensity),
            },
        });
    }
    Ok(Description {
        lamps,
        kind,
        min_update_interval_us,
        levels: levels.expect("at least one lamp"),
        programmable,
    })
}

/// An 8-bit channel scaled to a lamp's level count.
///
/// HID Usage Tables 1.4, section 26.9 (the LampAttributesResponseReport's
/// colour attributes): a count of zero means the channel is not supported,
/// one means fully on or off, and a count N means N levels "in addition to
/// being turned off", the highest being full brightness. So a count N takes
/// the values 0 (off) to N, not 0 to N−1. Microsoft's LampArray sample
/// (github.com/microsoft/ArduinoHidForWindows, `examples/LampArray`) reports
/// 0xFF and uses the 8-bit channel as it comes, which is the same reading.
pub fn scale(value: u8, count: u32) -> u32 {
    ((f64::from(value) * f64::from(count)) / 255.0).round() as u32
}

/// The intensity channel's value for a percentage: 0 (off) to the count,
/// the count being unity gain (section 26.9). A lamp that gives no count
/// is sent the field's own maximum, so it is never switched off by it.
pub fn scale_percent(percent: u8, count: u32, field: u32) -> u32 {
    let percent = percent.min(100);
    if count == 0 {
        field
    } else {
        ((f64::from(percent) * f64::from(count)) / 100.0).round() as u32
    }
}

/// Turns autonomous mode off and sets every lamp to one colour and
/// intensity in one range update.
pub fn set_all(
    device: &mut dyn FeatureDevice,
    layout: &Layout,
    description: &Description,
    colour: crate::lighting::Rgb,
    intensity_percent: u8,
) -> Result<(), LampError> {
    if !description.programmable {
        return Err(LampError::NotProgrammable);
    }
    if description.lamps == 0 {
        return Err(LampError::NoLamps);
    }
    let control = build(&layout.control, &[(usage::AUTONOMOUS_MODE, 0)]);
    device.set_feature(&control).map_err(LampError::Device)?;
    let report = &layout.range_update;
    let channel = |usage: u16| report.field(usage).map(|l| field_max(&l)).unwrap_or(0);
    let levels = description.levels;
    // `build` clamps each value to what its field carries.
    let update = build(
        report,
        &[
            // LampUpdateComplete: show it now.
            (usage::UPDATE_FLAGS, 1),
            (usage::LAMP_ID_START, 0),
            (usage::LAMP_ID_END, u32::from(description.lamps - 1)),
            (usage::RED_CHANNEL, scale(colour.red, levels.red)),
            (usage::GREEN_CHANNEL, scale(colour.green, levels.green)),
            (usage::BLUE_CHANNEL, scale(colour.blue, levels.blue)),
            (
                usage::INTENSITY_CHANNEL,
                scale_percent(
                    intensity_percent,
                    levels.intensity,
                    channel(usage::INTENSITY_CHANNEL),
                ),
            ),
        ],
    );
    if let Err(error) = device.set_feature(&update) {
        // Autonomous mode is off and no colour landed: the lamps would stay
        // on whatever they last showed. Give them back to the firmware once,
        // and report what failed first.
        let _ = give_back(device, layout);
        return Err(LampError::Device(error));
    }
    Ok(())
}

/// Turns autonomous mode on: the firmware draws its own lighting again.
pub fn give_back(device: &mut dyn FeatureDevice, layout: &Layout) -> Result<(), LampError> {
    let control = build(&layout.control, &[(usage::AUTONOMOUS_MODE, 1)]);
    device.set_feature(&control).map_err(LampError::Device)
}

/// A LampArray in memory, for tests and sandbox stand-ins.
#[cfg(any(test, feature = "test-support"))]
pub mod fake {
    use super::*;

    /// Answers like a LampArray with the M5406WA's report layout, and
    /// records what it was sent. Its knobs make it misbehave the ways a
    /// hostile or broken device could.
    #[derive(Clone, Debug)]
    pub struct FakeLampArray {
        pub layout: Layout,
        pub lamps: u16,
        pub kind: u32,
        pub levels: Levels,
        pub programmable: bool,
        pub autonomous: bool,
        /// The last colour set, as the device received it: R, G, B, I.
        pub shown: Option<[u32; 4]>,
        /// Every report sent, in order.
        pub sent: Vec<Vec<u8>>,
        selected: u32,
        /// Replies are cut to this many bytes.
        pub truncate_replies: Option<usize>,
        /// Lamp attributes answer for this lamp instead of the asked one.
        pub answer_lamp: Option<u32>,
        /// Every call fails with this.
        pub fail: Option<String>,
        /// Only the LampRangeUpdate report fails, with this.
        pub fail_range_update: Option<String>,
    }

    impl FakeLampArray {
        /// The owner's keyboard as far as it is known: one lamp, 255 levels
        /// per channel (the most an 8-bit level count can say). The real lamp count and levels are the owner's check.
        pub fn one_zone(descriptor: &[u8]) -> FakeLampArray {
            FakeLampArray {
                layout: Layout::from_descriptor(descriptor).expect("a LampArray descriptor"),
                lamps: 1,
                kind: 1,
                levels: Levels {
                    red: 255,
                    green: 255,
                    blue: 255,
                    intensity: 255,
                },
                programmable: true,
                autonomous: true,
                shown: None,
                sent: Vec::new(),
                selected: 0,
                truncate_replies: None,
                answer_lamp: None,
                fail: None,
                fail_range_update: None,
            }
        }
    }

    impl FeatureDevice for FakeLampArray {
        fn get_feature(&mut self, report: &mut [u8]) -> Result<usize, String> {
            if let Some(error) = &self.fail {
                return Err(error.clone());
            }
            let id = report[0];
            let layout = self.layout.clone();
            let filled = if id == layout.attributes.id {
                build(
                    &layout.attributes,
                    &[
                        (usage::LAMP_COUNT, u32::from(self.lamps)),
                        (usage::LAMP_ARRAY_KIND, self.kind),
                        (usage::MIN_UPDATE_INTERVAL, 16_000),
                    ],
                )
            } else if id == layout.response.id {
                let levels = self.levels;
                build(
                    &layout.response,
                    &[
                        (usage::LAMP_ID, self.answer_lamp.unwrap_or(self.selected)),
                        (usage::RED_LEVEL_COUNT, levels.red),
                        (usage::GREEN_LEVEL_COUNT, levels.green),
                        (usage::BLUE_LEVEL_COUNT, levels.blue),
                        (usage::INTENSITY_LEVEL_COUNT, levels.intensity),
                        (usage::IS_PROGRAMMABLE, u32::from(self.programmable)),
                    ],
                )
            } else {
                return Err(format!("no feature report {id:#04x}"));
            };
            let n = filled
                .len()
                .min(report.len())
                .min(self.truncate_replies.unwrap_or(usize::MAX));
            report[..n].copy_from_slice(&filled[..n]);
            Ok(n)
        }

        fn set_feature(&mut self, report: &[u8]) -> Result<(), String> {
            if let Some(error) = &self.fail {
                return Err(error.clone());
            }
            self.sent.push(report.to_vec());
            let layout = self.layout.clone();
            let field = |r: &Report, usage| {
                let l = r.field(usage).expect("a field");
                get_bits(&report[1..], l.bit_offset, l.bit_size).expect("in range")
            };
            let id = report[0];
            if id == layout.request.id && report.len() == layout.request.len {
                self.selected = field(&layout.request, usage::LAMP_ID);
            } else if id == layout.control.id && report.len() == layout.control.len {
                self.autonomous = field(&layout.control, usage::AUTONOMOUS_MODE) != 0;
            } else if id == layout.range_update.id && report.len() == layout.range_update.len {
                if let Some(error) = &self.fail_range_update {
                    return Err(error.clone());
                }
                if !self.autonomous {
                    let r = &layout.range_update;
                    self.shown = Some([
                        field(r, usage::RED_CHANNEL),
                        field(r, usage::GREEN_CHANNEL),
                        field(r, usage::BLUE_CHANNEL),
                        field(r, usage::INTENSITY_CHANNEL),
                    ]);
                }
            } else {
                return Err(format!(
                    "unexpected report {id:#04x} of {} bytes",
                    report.len()
                ));
            }
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeLampArray;
    use super::*;
    use crate::lighting::Rgb;

    /// `/sys/class/hidraw/hidraw1/device/report_descriptor` of the owner's
    /// ASUS Vivobook S 14 M5406WA (`ITE5570:00 0B05:5570`), read on
    /// 2026-11-21.
    const M5406WA: &[u8] = include_bytes!("../tests/fixtures/m5406wa-ite5570.rdesc");

    #[test]
    fn the_owners_keyboard_is_a_lamp_array_with_the_reports_the_research_found() {
        assert_eq!(M5406WA.len(), 748);
        let layout = Layout::from_descriptor(M5406WA).unwrap();
        assert_eq!(
            (
                layout.attributes.id,
                layout.request.id,
                layout.response.id,
                layout.range_update.id,
                layout.control.id
            ),
            (0x41, 0x42, 0x43, 0x45, 0x46)
        );
        assert_eq!(layout.attributes.len, 23);
        assert_eq!(layout.request.len, 3);
        assert_eq!(layout.response.len, 29);
        assert_eq!(layout.range_update.len, 10);
        assert_eq!(layout.control.len, 2);
        // Lamp count at byte 1, kind at byte 15, interval at byte 19.
        let at = |r: &Report, u| r.field(u).unwrap().bit_offset / 8 + 1;
        assert_eq!(at(&layout.attributes, usage::LAMP_COUNT), 1);
        assert_eq!(at(&layout.attributes, usage::LAMP_ARRAY_KIND), 15);
        assert_eq!(at(&layout.attributes, usage::MIN_UPDATE_INTERVAL), 19);
        assert_eq!(at(&layout.response, usage::RED_LEVEL_COUNT), 23);
        assert_eq!(at(&layout.response, usage::INTENSITY_LEVEL_COUNT), 26);
        assert_eq!(at(&layout.response, usage::IS_PROGRAMMABLE), 27);
    }

    #[test]
    fn the_vendor_copy_and_the_vendor_report_are_never_part_of_the_layout() {
        let fields = parse_descriptor(M5406WA).unwrap();
        // The vendor report 0x5F and the 0xFF59 copy (0x31-0x36) exist...
        assert!(fields.iter().any(|f| f.report_id == 0x5F));
        assert!(
            fields
                .iter()
                .any(|f| f.report_id == 0x31 && f.usage_page != LIGHTING_PAGE)
        );
        // ...and nothing Better OS builds uses them.
        let layout = Layout::from_descriptor(M5406WA).unwrap();
        for report in [
            &layout.attributes,
            &layout.request,
            &layout.response,
            &layout.range_update,
            &layout.control,
        ] {
            assert!((0x41..=0x46).contains(&report.id), "{:#04x}", report.id);
        }
        // The airplane-mode button's input report is not a lighting field.
        assert!(
            fields
                .iter()
                .any(|f| f.kind == ReportKind::Input && f.report_id == 4)
        );
    }

    #[test]
    fn the_owners_keyboard_described_set_and_given_back() {
        let mut device = FakeLampArray::one_zone(M5406WA);
        let layout = device.layout.clone();
        let description = describe(&mut device, &layout).unwrap();
        assert_eq!(description.lamps, 1);
        assert_eq!(description.kind, Some(LampArrayKind::Keyboard));
        assert_eq!(description.min_update_interval_us, Some(16_000));
        assert!(description.levels.has_colour());
        assert!(description.programmable);

        set_all(
            &mut device,
            &layout,
            &description,
            Rgb::new(255, 128, 0),
            100,
        )
        .unwrap();
        assert!(!device.autonomous);
        // A count of 255 is 255 levels and off: 0 to 255.
        assert_eq!(device.shown, Some([255, 128, 0, 255]));
        // Exactly: request, control off, range update.
        let ids: Vec<u8> = device.sent.iter().map(|r| r[0]).collect();
        assert_eq!(ids, vec![0x42, 0x46, 0x45]);
        assert_eq!(
            device.sent[2],
            vec![0x45, 0x01, 0x00, 0x00, 0x00, 0x00, 0xFF, 0x80, 0x00, 0xFF]
        );
        give_back(&mut device, &layout).unwrap();
        assert!(device.autonomous);
        assert_eq!(device.sent.last().unwrap(), &vec![0x46, 0x01]);
    }

    #[test]
    fn levels_are_scaled_to_what_the_lamps_have() {
        // HID Usage Tables 1.4, section 26.9: a count N is off plus N levels.
        assert_eq!(scale(255, 255), 255);
        assert_eq!(scale(128, 255), 128);
        assert_eq!(scale(255, 1), 1);
        assert_eq!(scale(127, 1), 0);
        assert_eq!(scale(128, 1), 1);
        assert_eq!(scale(255, 10), 10);
        assert_eq!(scale(0, 10), 0);
        // No such channel: nothing to send.
        assert_eq!(scale(200, 0), 0);
        assert_eq!(scale_percent(100, 4, 255), 4);
        assert_eq!(scale_percent(0, 4, 255), 0);
        assert_eq!(scale_percent(50, 4, 255), 2);
        assert_eq!(scale_percent(250, 4, 255), 4);
        // One level is on or off.
        assert_eq!(scale_percent(40, 1, 255), 0);
        assert_eq!(scale_percent(60, 1, 255), 1);
        // No count given: full, never off.
        assert_eq!(scale_percent(40, 0, 255), 255);

        let mut device = FakeLampArray::one_zone(M5406WA);
        device.levels = Levels {
            red: 4,
            green: 4,
            blue: 4,
            intensity: 0,
        };
        let layout = device.layout.clone();
        let description = describe(&mut device, &layout).unwrap();
        set_all(
            &mut device,
            &layout,
            &description,
            Rgb::new(255, 0, 170),
            10,
        )
        .unwrap();
        assert_eq!(device.shown, Some([4, 0, 3, 255]));
    }

    #[test]
    fn a_failed_colour_gives_the_lamps_back_and_reports_the_first_error() {
        let mut device = FakeLampArray::one_zone(M5406WA);
        let layout = device.layout.clone();
        let description = describe(&mut device, &layout).unwrap();
        device.fail_range_update = Some("EIO".into());
        assert_eq!(
            set_all(&mut device, &layout, &description, Rgb::new(1, 2, 3), 50),
            Err(LampError::Device("EIO".into()))
        );
        assert!(device.autonomous, "control went back to the firmware");
        let ids: Vec<u8> = device.sent.iter().map(|r| r[0]).collect();
        // The range update was tried and failed, then control went back.
        assert_eq!(ids, vec![0x42, 0x46, 0x45, 0x46]);
        assert_eq!(device.sent.last().unwrap(), &vec![0x46, 0x01]);
    }

    #[test]
    fn colour_needs_every_channel_and_intensity_more_than_on_and_off() {
        let levels = |red, green, blue, intensity| Levels {
            red,
            green,
            blue,
            intensity,
        };
        assert!(levels(1, 1, 1, 0).has_colour());
        assert!(!levels(255, 255, 0, 1).has_colour());
        assert!(!levels(0, 0, 0, 255).has_colour());
        assert!(levels(0, 0, 0, 2).has_intensity());
        assert!(!levels(255, 255, 255, 1).has_intensity());
    }

    #[test]
    fn many_lamps_share_the_fewest_levels_and_one_unprogrammable_lamp_refuses_the_write() {
        let mut device = FakeLampArray::one_zone(M5406WA);
        device.lamps = 101;
        let layout = device.layout.clone();
        let description = describe(&mut device, &layout).unwrap();
        assert_eq!(description.lamps, 101);
        set_all(&mut device, &layout, &description, Rgb::new(1, 2, 3), 50).unwrap();
        let update = device.sent.last().unwrap();
        // LampIdEnd is 100.
        assert_eq!(&update[4..6], &[100, 0]);

        let mut device = FakeLampArray::one_zone(M5406WA);
        device.programmable = false;
        let description = describe(&mut device, &layout).unwrap();
        assert!(!description.programmable);
        let before = device.sent.len();
        assert_eq!(
            set_all(&mut device, &layout, &description, Rgb::new(1, 2, 3), 50),
            Err(LampError::NotProgrammable)
        );
        assert_eq!(device.sent.len(), before);
    }

    #[test]
    fn a_hostile_device_is_refused_not_trusted() {
        let layout = Layout::from_descriptor(M5406WA).unwrap();
        let mut device = FakeLampArray::one_zone(M5406WA);
        device.lamps = 0;
        assert_eq!(describe(&mut device, &layout), Err(LampError::NoLamps));
        device.lamps = 60_000;
        assert_eq!(
            describe(&mut device, &layout),
            Err(LampError::TooManyLamps(60_000))
        );
        let mut device = FakeLampArray::one_zone(M5406WA);
        device.truncate_replies = Some(5);
        assert!(matches!(
            describe(&mut device, &layout),
            Err(LampError::BadReply {
                report: 0x41,
                got: 5
            })
        ));
        let mut device = FakeLampArray::one_zone(M5406WA);
        device.answer_lamp = Some(7);
        assert_eq!(
            describe(&mut device, &layout),
            Err(LampError::WrongLamp { asked: 0, got: 7 })
        );
        let mut device = FakeLampArray::one_zone(M5406WA);
        device.fail = Some("EIO".into());
        assert_eq!(
            describe(&mut device, &layout),
            Err(LampError::Device("EIO".into()))
        );
    }

    #[test]
    fn every_truncation_of_the_real_descriptor_is_refused_or_parsed_without_panicking() {
        // The LampArray collection closes at byte 420; after it comes the
        // vendor copy on 0xFF59 up to byte 748.
        let lamp_end = 420;
        assert_eq!(&M5406WA[lamp_end - 2..lamp_end], &[0xC0, 0xC0]);
        let full = Layout::from_descriptor(M5406WA).unwrap();
        assert!(Layout::from_descriptor(&M5406WA[..M5406WA.len() - 1]).is_err());
        for end in 0..M5406WA.len() {
            let parsed = Layout::from_descriptor(&M5406WA[..end]);
            if end < lamp_end {
                assert!(parsed.is_err(), "cut at {end}");
            } else if end == lamp_end {
                assert!(parsed.is_ok(), "cut at {end}");
            } else {
                // Past the LampArray: refused once the vendor collection is
                // open and left unbalanced, and otherwise the same layout.
                if let Ok(layout) = parsed {
                    assert_eq!(layout, full, "cut at {end}");
                }
            }
        }
    }

    #[test]
    fn hostile_descriptors_are_refused() {
        // Too long.
        assert_eq!(
            parse_descriptor(&vec![0u8; MAX_DESCRIPTOR_LEN + 1]),
            Err(DescriptorError::TooLong(MAX_DESCRIPTOR_LEN + 1))
        );
        assert_eq!(parse_descriptor(&[]), Err(DescriptorError::Empty));
        // An item whose data runs off the end.
        assert_eq!(
            parse_descriptor(&[0x27, 0xFF, 0xFF]),
            Err(DescriptorError::Truncated { at: 0 })
        );
        // A long item claiming more bytes than there are.
        assert_eq!(
            parse_descriptor(&[0xFE, 0xFF, 0x00]),
            Err(DescriptorError::Truncated { at: 0 })
        );
        // Collections nested past the limit.
        let deep: Vec<u8> = std::iter::repeat_n([0xA1, 0x01], 40).flatten().collect();
        assert_eq!(parse_descriptor(&deep), Err(DescriptorError::TooDeep));
        // An end of collection with none open, and one left open.
        assert!(matches!(
            parse_descriptor(&[0xC0]),
            Err(DescriptorError::UnbalancedCollection { .. })
        ));
        assert!(matches!(
            parse_descriptor(&[0x05, 0x59, 0x09, 0x01, 0xA1, 0x01]),
            Err(DescriptorError::UnbalancedCollection { .. })
        ));
        // A report far larger than the kernel moves: size 256 x count 200.
        assert!(matches!(
            parse_descriptor(&[
                0x85, 0x01, 0x75, 0xFF, 0x96, 0xC8, 0x00, 0x09, 0x01, 0xB1, 0x02
            ]),
            Err(DescriptorError::BadReportSize { .. } | DescriptorError::ReportTooLong { .. })
        ));
        // Report size 0, report id 0, pops with nothing pushed.
        assert!(parse_descriptor(&[0x75, 0x00]).is_err());
        assert!(parse_descriptor(&[0x85, 0x00]).is_err());
        assert!(parse_descriptor(&[0xB4]).is_err());
        // Pushes past the limit.
        assert!(parse_descriptor(&[0xA4; 20]).is_err());
    }

    #[test]
    fn a_lamp_array_without_a_report_it_needs_is_not_driven() {
        // The owner's descriptor with the LampArrayControl report's usage
        // (0x70) changed to an unknown one: no control, so no way to take
        // or give back control, so not a LampArray Better OS drives.
        let mut bytes = M5406WA.to_vec();
        let at = bytes
            .windows(4)
            .position(|w| w == [0x85, 0x46, 0x09, 0x70])
            .unwrap();
        bytes[at + 3] = 0x7F;
        assert_eq!(
            Layout::from_descriptor(&bytes),
            Err(LayoutError::MissingReport(usage::CONTROL_REPORT))
        );
        // A mouse descriptor is no LampArray at all.
        let mouse = [
            0x05, 0x01, 0x09, 0x02, 0xA1, 0x01, 0x09, 0x01, 0xA1, 0x00, 0x05, 0x09, 0x19, 0x01,
            0x29, 0x03, 0x15, 0x00, 0x25, 0x01, 0x95, 0x03, 0x75, 0x01, 0x81, 0x02, 0x95, 0x01,
            0x75, 0x05, 0x81, 0x03, 0xC0, 0xC0,
        ];
        assert!(parse_descriptor(&mouse).is_ok());
        assert_eq!(
            Layout::from_descriptor(&mouse),
            Err(LayoutError::NotALampArray)
        );
        assert!(!is_lamp_array(&mouse));
        assert!(is_lamp_array(M5406WA));
    }

    #[test]
    fn random_bytes_never_panic() {
        // A small deterministic generator: no dependency, the same bytes
        // every run.
        let mut state = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..2000 {
            let len = (state % 300) as usize;
            let mut bytes = Vec::with_capacity(len);
            for _ in 0..len {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                bytes.push(state as u8);
            }
            let _ = Layout::from_descriptor(&bytes);
            // Mutations of the real descriptor too.
            let mut real = M5406WA.to_vec();
            let i = (state as usize) % real.len();
            real[i] = (state >> 8) as u8;
            let _ = Layout::from_descriptor(&real);
        }
    }

    #[test]
    fn bits_outside_the_report_are_none() {
        assert_eq!(get_bits(&[0xFF], 0, 8), Some(0xFF));
        assert_eq!(get_bits(&[0xFF], 4, 8), None);
        assert_eq!(get_bits(&[0x34, 0x12], 0, 16), Some(0x1234));
        assert_eq!(get_bits(&[0b1010_0000], 5, 3), Some(0b101));
        assert_eq!(get_bits(&[0; 8], 0, 33), None);
        assert_eq!(get_bits(&[0; 8], u32::MAX, 8), None);
    }
}
