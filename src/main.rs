use ropey::Rope;
use std::{
    fs::File,
    io::{BufReader, Stdout, Write, stdin, stdout},
};
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
    file_text: Rope,
    cursor_position: CursorPosition,
    term_size: TermSize,
}

impl EditorState {
    fn new(path: String) -> Self {
        let (cols, rows) = termion::terminal_size().unwrap();

        let file_text = Rope::from_reader(BufReader::new(File::open(path).unwrap())).unwrap();

        EditorState {
            file_text,
            cursor_position: CursorPosition { col: 1, row: 1 },
            term_size: TermSize { cols, rows },
        }
    }

    fn cursor_left(&mut self) {
        let in_first_col = self.cursor_position.col == 1;
        let in_first_row = self.cursor_position.row == 1;

        if !in_first_col {
            self.cursor_position.col -= 1;
        } else if !in_first_row {
            self.cursor_position.row -= 1;
            self.cursor_position.col = self.term_size.cols;
        }
    }

    fn cursor_right(&mut self) {
        let in_last_col = self.cursor_position.col == self.term_size.cols;
        let in_last_row = self.cursor_position.row == self.term_size.rows;

        if !in_last_col {
            self.cursor_position.col += 1;
        } else if !in_last_row {
            self.cursor_position.row += 1;
            self.cursor_position.col = 1;
        }
    }

    fn cursor_up(&mut self) {
        if self.cursor_position.row != 1 {
            self.cursor_position.row -= 1;
        }
    }

    fn cursor_down(&mut self) {
        if self.cursor_position.row != self.term_size.rows {
            self.cursor_position.row += 1;
        }
    }
}

fn process_keypress(key: Key, state: &mut EditorState) -> bool {
    match key {
        Key::Char('q') => return true,
        Key::Left => state.cursor_left(),
        Key::Right => state.cursor_right(),
        Key::Up => state.cursor_up(),
        Key::Down => state.cursor_down(),
        _ => (),
    }

    return false;
}

fn render_editor(state: &EditorState, term: &mut Term) {
    write!(term, "{}", termion::clear::All).unwrap();

    for (line_num, line_text) in state
        .file_text
        .lines()
        .enumerate()
        .take(state.term_size.rows.into())
    {
        write!(
            term,
            "{}{}",
            termion::cursor::Goto(1, (line_num + 1).try_into().unwrap()),
            line_text
        )
        .unwrap();
    }

    write!(
        term,
        "{}",
        termion::cursor::Goto(state.cursor_position.col, state.cursor_position.row)
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

    let path = std::env::args().skip(1).next().unwrap();

    let mut state = EditorState::new(path);

    render_editor(&state, &mut term);

    for c in stdin.keys() {
        let quit = process_keypress(c.unwrap(), &mut state);

        if quit {
            break;
        } else {
            render_editor(&state, &mut term);
        }
    }
}
