use std::io::{Stdout, Write, stdin, stdout};
use termion::{
    event::Key,
    input::TermRead,
    raw::{IntoRawMode, RawTerminal},
    screen::{AlternateScreen, IntoAlternateScreen},
};

type Term = AlternateScreen<RawTerminal<Stdout>>;

struct CursorPosition {
    col: u16,
    row: u16,
}

struct TermSize {
    cols: u16,
    rows: u16,
}

struct EditorState {
    cursor_position: CursorPosition,
    term_size: TermSize,
}

impl EditorState {
    fn new() -> Self {
        let (cols, rows) = termion::terminal_size().unwrap();

        EditorState {
            cursor_position: CursorPosition { col: 0, row: 0 },
            term_size: TermSize { cols, rows },
        }
    }
}

fn process_keypress(key: Key, state: &mut EditorState) -> bool {
    match key {
        Key::Char('q') => return true,
        Key::Left => state.cursor_position.col -= 1,
        Key::Right => state.cursor_position.col += 1,
        Key::Up => state.cursor_position.row -= 1,
        Key::Down => state.cursor_position.row += 1,
        _ => (),
    }

    return false;
}

fn render_editor(state: &EditorState, term: &mut Term) {
    write!(
        term,
        "{}",
        termion::cursor::Goto(state.cursor_position.col + 1, state.cursor_position.row + 1)
    )
    .unwrap();
    term.flush().unwrap();
}

fn main() {
    let stdin = stdin();
    let mut term = stdout()
        .into_raw_mode()
        .unwrap()
        .into_alternate_screen()
        .unwrap();

    let mut state = EditorState::new();

    write!(
        term,
        "{}q to exit{}",
        termion::clear::All,
        termion::cursor::Goto(1, 1),
        // termion::cursor::Hide
    )
    .unwrap();
    term.flush().unwrap();

    for c in stdin.keys() {
        let quit = process_keypress(c.unwrap(), &mut state);

        if quit {
            break;
        } else {
            render_editor(&state, &mut term);
        }
    }
}
