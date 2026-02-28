/// A placeholder that does nothing of consequence.
pub fn placeholder() -> &'static str {
    "nothing to see here"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_returns_something() {
        assert_eq!(placeholder(), "nothing to see here");
    }
}
