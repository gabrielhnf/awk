use std::{
    borrow::Cow,
    fmt::Display,
    hash::Hash,
    io::Write,
    mem::discriminant,
    ops::{Add, Div, Mul, Neg, Not, Rem, Sub},
};

use ahash::RandomState;
use hashbrown::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value<'a> {
    Float(f64),
    String(Cow<'a, [u8]>),
    Regex(Cow<'a, [u8]>),
    Array(HashMap<String, Self, RandomState>),
    Bool(bool),
    Untyped,
    Unassigned,
}

impl Value<'_> {
    /// Called when loading a variable's value. Forces subsequent uses to be
    /// typed as an AWK scalar (anything that's not an array, basically).
    pub fn scalar_context(&mut self) -> &mut Self {
        // TODO: Exit "nicely" on Self::Array(_).
        match self {
            Self::Untyped => *self = Self::Unassigned,
            Self::Array(_) => panic!("Attempted to use array in scalar context!"),
            _ => {}
        }
        self
    }

    pub fn array_context(&mut self) -> &mut Self {
        // TODO: Exit "nicely" on Self::Array(_).
        match self {
            Self::Untyped => *self = Self::Array(HashMap::with_hasher(RandomState::new())),
            Self::Array(_) => {}
            _ => panic!("Attempted to use scalar as array!"),
        }
        self
    }

    pub fn to_bool(&self) -> bool {
        match self {
            &Self::Float(f) => f != 0.,
            &Self::Bool(b) => b,
            Self::String(str) => !str.is_empty(),
            _ => false,
        }
    }

    fn to_num(&self) -> f64 {
        match self {
            &Self::Float(f) => f,
            &Self::Bool(b) => b as usize as f64,
            Self::String(s) => str::from_utf8(s)
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0.),
            _ => 0.,
        }
    }

    pub fn pow(&self, rhs: &Self) -> Self {
        Value::Float(self.to_num().powf(rhs.to_num()))
    }

    pub fn to_int(&self) -> Self {
        Value::Float(self.to_num().trunc())
    }

    pub fn write_string(&self, f: &mut Vec<u8>) {
        match self {
            Self::String(s) | Self::Regex(s) => f.extend_from_slice(s),
            Self::Float(n) => write!(f, "{n}").unwrap(),
            &Self::Bool(false) => f.push(b'0'),
            &Self::Bool(true) => f.push(b'1'),
            Self::Array(_) => panic!("Attempted to use array in scalar context!"),
            _ => {}
        }
    }

    pub fn string_size_hint(&self) -> usize {
        match self {
            Self::String(s) | Self::Regex(s) => s.len(),
            Self::Float(_) => 2,
            Self::Bool(_) => 1,
            _ => 0,
        }
    }
}

impl<'a> Add for &'_ Value<'a> {
    type Output = Value<'a>;

    fn add(self, rhs: Self) -> Self::Output {
        Value::Float(self.to_num() + rhs.to_num())
    }
}

impl<'a> Sub for &'_ Value<'a> {
    type Output = Value<'a>;

    fn sub(self, rhs: Self) -> Self::Output {
        Value::Float(self.to_num() - rhs.to_num())
    }
}

impl<'a> Mul for &'_ Value<'a> {
    type Output = Value<'a>;

    fn mul(self, rhs: Self) -> Self::Output {
        Value::Float(self.to_num() * rhs.to_num())
    }
}

impl<'a> Div for &'_ Value<'a> {
    type Output = Value<'a>;

    fn div(self, rhs: Self) -> Self::Output {
        // TODO: panic "nicely" on div by zero.
        Value::Float(self.to_num() / rhs.to_num())
    }
}

impl<'a> Rem for &'_ Value<'a> {
    type Output = Value<'a>;

    fn rem(self, rhs: Self) -> Self::Output {
        Value::Float(self.to_num() % rhs.to_num())
    }
}

impl<'a> Neg for &'_ Value<'a> {
    type Output = Value<'a>;

    fn neg(self) -> Self::Output {
        Value::Float(-self.to_num())
    }
}

impl<'a> Not for &'_ Value<'a> {
    type Output = Value<'a>;

    fn not(self) -> Self::Output {
        Value::Bool(!self.to_bool())
    }
}

impl Eq for Value<'_> {}
impl PartialOrd for Value<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        match (self, other) {
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
            (Value::Bool(a), Value::Bool(b)) => a.partial_cmp(b),
            (Value::String(a), Value::String(b)) => a.partial_cmp(b),
            //TODO: handle mixed type comparisons following awk's coercion rules
            // (e.g. numeric strings should compare numerically, Unassigned is "" or 0 depending on
            // context)
            _ => None,
        }
    }
}
impl Hash for Value<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        discriminant(self).hash(state);
        match self {
            Self::Float(f) => state.write_u64(f.to_bits()),
            Self::String(s) => s.hash(state),
            Self::Bool(b) => b.hash(state),
            _ => {}
        }
    }
}

impl Display for Value<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Float(n) => <_ as Display>::fmt(n, f),
            Value::String(s) => write!(f, "{:?}", String::from_utf8_lossy(s)),
            Value::Regex(s) => write!(f, "/{}/", String::from_utf8_lossy(s)),
            &Value::Bool(b) => write!(f, "{}", b as usize),
            Value::Array(_) => write!(f, "array"),
            Value::Untyped => write!(f, "untyped"),
            Value::Unassigned => write!(f, "unassigned"),
        }
    }
}
