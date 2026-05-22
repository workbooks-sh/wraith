use serde::Serialize;

#[derive(Serialize)]
pub struct UsedStruct {
    pub field: u32,
}

/// referenced by crate-b
pub fn used_function() -> UsedStruct {
    UsedStruct { field: 42 }
}

/// never referenced anywhere → should appear as dead code
pub fn truly_dead_function() -> i32 {
    7
}

/// also never referenced
pub struct OrphanStruct {
    pub x: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke() {
        let _ = used_function();
    }
}
