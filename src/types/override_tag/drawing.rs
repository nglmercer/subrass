/// Check whether a vector clip contains at least one supported ASS drawing
/// command. Keeping this lexer predicate separate prevents clip parsing from
/// growing a second drawing grammar.
pub(super) fn drawing_contains_command(drawing: &str) -> bool {
    drawing.chars().any(|c| {
        matches!(
            c,
            'm' | 'n' | 'l' | 'b' | 's' | 'p' | 'c' | 'M' | 'N' | 'L' | 'B' | 'S' | 'P' | 'C'
        )
    })
}
