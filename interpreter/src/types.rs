use std::{
    hash::Hash,
    mem::discriminant,
    ops::{Add, Div, Mul, Sub},
};

use ahash::RandomState;
use hashbrown::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Float(f64),
    String(Vec<u8>),
    Array(HashMap<String, Self, RandomState>),
    Bool(bool),
    Untyped,
    Unassigned,
}

impl Value {
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
}

impl Add for &'_ Value {
    type Output = Value;

    fn add(self, rhs: Self) -> Self::Output {
        Value::Float(self.to_num() + rhs.to_num())
    }
}

impl Sub for &'_ Value {
    type Output = Value;

    fn sub(self, rhs: Self) -> Self::Output {
        Value::Float(self.to_num() - rhs.to_num())
    }
}

impl Mul for &'_ Value {
    type Output = Value;

    fn mul(self, rhs: Self) -> Self::Output {
        Value::Float(self.to_num() * rhs.to_num())
    }
}

impl Div for &'_ Value {
    type Output = Value;

    fn div(self, rhs: Self) -> Self::Output {
        // TODO: panic "nicely" on div by zero.
        Value::Float(self.to_num() / rhs.to_num())
    }
}

impl Eq for Value {}
impl Hash for Value {
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
