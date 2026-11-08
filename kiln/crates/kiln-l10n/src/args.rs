// SPDX-License-Identifier: Apache-2.0

//! A message's named arguments, owned, so a [`Text`](crate::Text) can keep
//! them until it is resolved.

use std::borrow::Cow;

use fluent_bundle::types::FluentNumber;
use fluent_bundle::{FluentArgs, FluentValue};
use gpui_shared_string::SharedString;

/// One argument's value: a string, or a number for plural selectors and
/// number formatting.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Str(SharedString),
    Number(FluentNumber),
}

/// A value [`tr!`](crate::tr) and [`text!`](crate::text) take as an argument.
pub trait IntoArg {
    fn into_arg(self) -> Arg;
}

impl IntoArg for Arg {
    fn into_arg(self) -> Arg {
        self
    }
}

impl IntoArg for &str {
    fn into_arg(self) -> Arg {
        Arg::Str(SharedString::from(self))
    }
}

impl IntoArg for String {
    fn into_arg(self) -> Arg {
        Arg::Str(SharedString::from(self))
    }
}

impl IntoArg for &String {
    fn into_arg(self) -> Arg {
        Arg::Str(SharedString::from(self))
    }
}

impl IntoArg for SharedString {
    fn into_arg(self) -> Arg {
        Arg::Str(self)
    }
}

impl IntoArg for &SharedString {
    fn into_arg(self) -> Arg {
        Arg::Str(self.clone())
    }
}

impl IntoArg for Cow<'_, str> {
    fn into_arg(self) -> Arg {
        Arg::Str(SharedString::from(self))
    }
}

impl IntoArg for char {
    fn into_arg(self) -> Arg {
        Arg::Str(SharedString::from(self))
    }
}

macro_rules! number_args {
    ($($num:ty)+) => {
        $(
            impl IntoArg for $num {
                fn into_arg(self) -> Arg {
                    Arg::Number(FluentNumber::from(self))
                }
            }

            impl IntoArg for &$num {
                fn into_arg(self) -> Arg {
                    Arg::Number(FluentNumber::from(*self))
                }
            }
        )+
    };
}

number_args!(i8 i16 i32 i64 i128 isize u8 u16 u32 u64 u128 usize f32 f64);

/// A message's named arguments, in the order they were given.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args(Vec<(Cow<'static, str>, Arg)>);

impl Args {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `name`, replacing an argument of the same name.
    pub fn with(mut self, name: impl Into<Cow<'static, str>>, value: impl IntoArg) -> Self {
        self.set(name, value);
        self
    }

    /// Sets `name`, replacing an argument of the same name.
    pub fn set(&mut self, name: impl Into<Cow<'static, str>>, value: impl IntoArg) {
        let name = name.into();
        let value = value.into_arg();
        match self.0.iter_mut().find(|(n, _)| *n == name) {
            Some(slot) => slot.1 = value,
            None => self.0.push((name, value)),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Arg> {
        self.0
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, value)| value)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Arg)> {
        self.0.iter().map(|(name, value)| (name.as_ref(), value))
    }

    /// The arguments as Fluent takes them, borrowing the strings. Values are
    /// passed raw: isolation happens in the bundle's formatter, where a value
    /// is written into the text, so selectors match the value itself.
    pub(crate) fn to_fluent(&self) -> FluentArgs<'_> {
        let mut args = FluentArgs::with_capacity(self.0.len());
        for (name, value) in &self.0 {
            let value = match value {
                Arg::Str(s) => FluentValue::String(Cow::Borrowed(s.as_str())),
                Arg::Number(n) => FluentValue::Number(n.clone()),
            };
            args.set(name.as_ref(), value);
        }
        args
    }
}
