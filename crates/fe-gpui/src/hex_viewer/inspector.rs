//! The byte inspector's values, without GPUI: the numbers the bytes at the
//! selection read as, in one byte order.

/// Bytes the widest type reads.
pub(crate) const INSPECT_BYTES: usize = 8;

/// A byte order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Endian {
    #[default]
    Little,
    Big,
}

impl Endian {
    /// The other order.
    pub(crate) fn flipped(self) -> Self {
        match self {
            Self::Little => Self::Big,
            Self::Big => Self::Little,
        }
    }
}

/// What the bytes read as, one value per type, `None` where too few bytes
/// are left.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Inspection {
    pub(crate) u8: Option<u8>,
    pub(crate) i8: Option<i8>,
    pub(crate) u16: Option<u16>,
    pub(crate) i16: Option<i16>,
    pub(crate) u32: Option<u32>,
    pub(crate) i32: Option<i32>,
    pub(crate) u64: Option<u64>,
    pub(crate) i64: Option<i64>,
    pub(crate) f32: Option<f32>,
    pub(crate) f64: Option<f64>,
}

/// The first `N` bytes of `bytes`, if there are that many.
fn head<const N: usize>(bytes: &[u8]) -> Option<[u8; N]> {
    bytes.get(..N)?.try_into().ok()
}

/// `bytes`' first bytes read as each type in `endian`.
pub(crate) fn inspect(bytes: &[u8], endian: Endian) -> Inspection {
    macro_rules! read {
        ($ty:ty, $n:literal) => {
            head::<$n>(bytes).map(|b| match endian {
                Endian::Little => <$ty>::from_le_bytes(b),
                Endian::Big => <$ty>::from_be_bytes(b),
            })
        };
    }
    Inspection {
        u8: read!(u8, 1),
        i8: read!(i8, 1),
        u16: read!(u16, 2),
        i16: read!(i16, 2),
        u32: read!(u32, 4),
        i32: read!(i32, 4),
        u64: read!(u64, 8),
        i64: read!(i64, 8),
        f32: read!(f32, 4),
        f64: read!(f64, 8),
    }
}

impl Inspection {
    /// Each type's name and its value as text, `None` if missing. Floats
    /// print as Rust's `{}` does.
    pub(crate) fn rows(&self) -> [(&'static str, Option<String>); 10] {
        fn text<T: std::fmt::Display>(value: Option<T>) -> Option<String> {
            value.map(|value| value.to_string())
        }
        [
            ("u8", text(self.u8)),
            ("i8", text(self.i8)),
            ("u16", text(self.u16)),
            ("i16", text(self.i16)),
            ("u32", text(self.u32)),
            ("i32", text(self.i32)),
            ("u64", text(self.u64)),
            ("i64", text(self.i64)),
            ("f32", text(self.f32)),
            ("f64", text(self.f64)),
        ]
    }
}
