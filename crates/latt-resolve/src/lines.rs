//! The lines of a file and where each starts, so a place costs the same in a file of any length:
//! generated files have tens of thousands of lines, with an occurrence on most of them.

use std::ops::Deref;

pub struct Lines<'a> {
    text: Vec<&'a str>,
    starts: Vec<u32>,
}

impl<'a> Lines<'a> {
    pub fn new(source: &'a str) -> Self {
        let text: Vec<&str> = source.split('\n').collect();
        let mut starts = Vec::with_capacity(text.len() + 1);
        let mut next = 0;
        starts.push(next);
        for line in &text {
            next += line.len() as u32 + 1;
            starts.push(next);
        }
        Self { text, starts }
    }

    /// The byte offset of a place. A line past the end counts as the end.
    pub fn offset(&self, line: u32, column: u32) -> u32 {
        let line = (line as usize).min(self.text.len());
        self.starts[line] + column
    }
}

impl<'a> Deref for Lines<'a> {
    type Target = [&'a str];

    fn deref(&self) -> &Self::Target {
        &self.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_is_its_byte_offset() {
        let lines = Lines::new("ab\n\nфг\nd");
        assert_eq!(lines.offset(0, 1), 1);
        assert_eq!(lines.offset(1, 0), 3);
        assert_eq!(lines.offset(2, 2), 6);
        assert_eq!(lines.offset(3, 1), 10);
        assert_eq!(lines.offset(9, 0), 11, "past the end");
    }
}
