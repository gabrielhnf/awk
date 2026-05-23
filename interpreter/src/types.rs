use std::ptr::NonNull;

// / NaN-boxed value
union AValue {
    float: f64,
    ptr: NonNull<()>,
}

impl AValue {
    fn get_float(&self) -> f64 {
        let float = unsafe { self.float };
        if !float.is_nan() { float } else { todo!() }
    }
}
