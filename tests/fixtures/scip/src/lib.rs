pub mod left {
    pub fn duplicate() -> i32 {
        1
    }
}
pub mod right {
    pub fn duplicate() -> i32 {
        2
    }
}
pub fn selected() -> i32 {
    left::duplicate() + right::duplicate()
}
