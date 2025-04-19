use ropey::Rope;
use std::{
    fs::File,
    io::{self, BufReader, Stdout, Write, stdin, stdout},
};
use termion::{
    event::Key,
    input::TermRead,
    raw::{IntoRawMode, RawTerminal},
    screen::{AlternateScreen, IntoAlternateScreen},
};

const SCROLLOFF: usize = 5;

type Term = AlternateScreen<RawTerminal<Stdout>>;

struct CursorPosition {
    /// Ropey char_idx (char in file cursor is on)
    char: usize,
    /// if a vertical move causes a change in cursor column (because the new line is too short)
    /// this stores the original column so it can be restored on a subsequent vertical move
    target_col: Option<u16>,
}

struct TermSize {
    // TODO: remove
    #[allow(dead_code)]
    cols: u16,
    rows: u16,
}

struct EditorState {
    file_text: Rope,
    /// first line in file to draw
    view_offset: usize,
    cursor_position: CursorPosition,
    term_size: TermSize,
}

impl EditorState {
    fn new(path: String) -> Self {
        let (cols, rows) = termion::terminal_size().unwrap();

        let file_text = Rope::from_reader(BufReader::new(File::open(path).unwrap())).unwrap();

        EditorState {
            file_text,
            view_offset: 0,
            cursor_position: CursorPosition {
                char: 0,
                target_col: None,
            },
            term_size: TermSize { cols, rows },
        }
    }

    fn get_cursor_visual_pos(&self) -> (u16, u16) {
        let line = self.file_text.char_to_line(self.cursor_position.char);
        let line_start = self.file_text.line_to_char(line);

        let col = ((self.cursor_position.char - line_start) + 1)
            .try_into()
            .unwrap();
        let row = (line - self.view_offset + 1).try_into().unwrap();

        (col, row)
    }

    fn cursor_left(&mut self) {
        self.cursor_position.char = self.cursor_position.char.saturating_sub(1);
        self.cursor_position.target_col = None
    }

    fn cursor_right(&mut self) {
        self.cursor_position.char =
            (self.cursor_position.char + 1).min(self.file_text.len_chars() - 1);
        self.cursor_position.target_col = None
    }

    fn cursor_up(&mut self) {
        let curr_line = self.file_text.char_to_line(self.cursor_position.char);
        if curr_line < 1 {
            return;
        }

        let start_of_curr_line = self.file_text.line_to_char(curr_line);
        let start_of_prev_line = self.file_text.line_to_char(curr_line - 1);
        let end_of_prev_line = start_of_curr_line - 1;

        let curr_col: u16 = (self.cursor_position.char - start_of_curr_line)
            .try_into()
            .unwrap();
        let target_col = self
            .cursor_position
            .target_col
            .map_or(curr_col, |target_col| target_col.max(curr_col));

        let optimistic_next_pos = start_of_prev_line + target_col as usize;

        self.cursor_position = if optimistic_next_pos > end_of_prev_line {
            CursorPosition {
                char: end_of_prev_line,
                target_col: Some(target_col),
            }
        } else {
            CursorPosition {
                char: optimistic_next_pos,
                target_col: None,
            }
        }
    }

    // TODO: DRY potential with cursor_up
    fn cursor_down(&mut self) {
        let curr_line = self.file_text.char_to_line(self.cursor_position.char);
        if !(curr_line < self.file_text.len_lines()) {
            return;
        }

        let start_of_curr_line = self.file_text.line_to_char(curr_line);
        let start_of_next_line = self.file_text.line_to_char(curr_line + 1);
        let end_of_next_line = self.file_text.line_to_char(curr_line + 2) - 1;

        let curr_col: u16 = (self.cursor_position.char - start_of_curr_line)
            .try_into()
            .unwrap();
        let target_col = self
            .cursor_position
            .target_col
            .map_or(curr_col, |target_col| target_col.max(curr_col));

        let optimistic_next_pos = start_of_next_line + target_col as usize;

        self.cursor_position = if optimistic_next_pos > end_of_next_line {
            CursorPosition {
                char: end_of_next_line,
                target_col: Some(target_col),
            }
        } else {
            CursorPosition {
                char: optimistic_next_pos,
                target_col: None,
            }
        }
    }

    fn ensure_cursor_in_view(&mut self) {
        let line = self.file_text.char_to_line(self.cursor_position.char);
        let delta: isize = line as isize - self.view_offset as isize;

        if line < SCROLLOFF {
            return;
        }

        if delta < SCROLLOFF as isize {
            self.view_offset -= (SCROLLOFF as isize - delta) as usize;
        } else if delta > (self.term_size.rows as isize - SCROLLOFF as isize) {
            self.view_offset +=
                (delta - (self.term_size.rows as isize - SCROLLOFF as isize)) as usize
        }
    }
}

fn init_tui() -> io::Result<Term> {
    let mut term = stdout().into_raw_mode()?;
    write!(term, "{}", termion::cursor::Save)?;
    term.flush()?;
    term.into_alternate_screen()
}

fn restore_tui() -> io::Result<()> {
    write!(
        stdout(),
        "{}{}",
        termion::screen::ToMainScreen,
        termion::cursor::Restore
    )?;
    stdout().flush()?;
    Ok(())
}

fn init_panic_hook() -> io::Result<()> {
    let raw_output = stdout().into_raw_mode()?;
    raw_output.suspend_raw_mode()?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // intentionally ignore errors here since we're already in a panic
        let _ = raw_output.suspend_raw_mode();
        let _ = restore_tui();
        original_hook(panic_info);
    }));
    Ok(())
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
        .skip(state.view_offset)
        // remove trailing newline
        .map(|line| line.slice(..(line.len_chars() - 1)))
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

    let (col, row) = state.get_cursor_visual_pos();
    write!(term, "{}", termion::cursor::Goto(col, row)).unwrap();

    term.flush().unwrap();
}

fn main() {
    init_panic_hook().unwrap();

    let stdin = stdin();
    let mut term = init_tui().unwrap();

    let path = std::env::args().skip(1).next().unwrap();

    let mut state = EditorState::new(path);

    render_editor(&state, &mut term);

    for c in stdin.keys() {
        let quit = process_keypress(c.unwrap(), &mut state);

        state.ensure_cursor_in_view();

        if quit {
            break;
        } else {
            render_editor(&state, &mut term);
        }
    }
}
