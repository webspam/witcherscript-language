use tree_sitter::Point;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

#[derive(Debug, Clone)]
pub struct LineIndex {
    line_starts: Vec<usize>,
}

impl LineIndex {
    pub fn new(source: &str) -> Self {
        let mut line_starts = vec![0];

        for (index, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                line_starts.push(index + 1);
            }
        }

        Self { line_starts }
    }

    pub fn point_to_position(&self, point: Point) -> SourcePosition {
        SourcePosition {
            line: point.row as u32,
            character: point.column as u32,
        }
    }

    pub fn byte_to_position(&self, source: &str, byte: usize) -> SourcePosition {
        let line_index = self
            .line_starts
            .partition_point(|line_start| *line_start <= byte)
            .saturating_sub(1);
        let line_start = self.line_starts[line_index];
        let character = source[line_start..byte]
            .encode_utf16()
            .count()
            .try_into()
            .unwrap_or(u32::MAX);

        SourcePosition {
            line: line_index.try_into().unwrap_or(u32::MAX),
            character,
        }
    }

    pub fn position_to_byte(&self, source: &str, position: SourcePosition) -> Option<usize> {
        let line_start = *self.line_starts.get(position.line as usize)?;
        let line_end = self
            .line_starts
            .get(position.line as usize + 1)
            .copied()
            .unwrap_or(source.len());
        let line = source.get(line_start..line_end)?;
        let mut utf16_units = 0;

        for (offset, character) in line.char_indices() {
            if utf16_units == position.character {
                return Some(line_start + offset);
            }
            utf16_units += character.len_utf16() as u32;
            if utf16_units > position.character {
                return None;
            }
        }

        if utf16_units == position.character {
            Some(line_end)
        } else {
            None
        }
    }

    pub fn point_range_to_range(&self, start: Point, end: Point) -> SourceRange {
        SourceRange {
            start: self.point_to_position(start),
            end: self.point_to_position(end),
        }
    }

    pub fn byte_range_to_range(
        &self,
        source: &str,
        start_byte: usize,
        end_byte: usize,
    ) -> SourceRange {
        SourceRange {
            start: self.byte_to_position(source, start_byte),
            end: self.byte_to_position(source, end_byte),
        }
    }
}

#[cfg(test)]
mod tests;
